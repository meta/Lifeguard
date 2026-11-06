/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Immutable mutation queries, parameter fixpoints, and deferred cross-library candidates.

use dashmap::DashMap;
use pyrefly_python::module_name::ModuleName;
use rayon::prelude::*;

use crate::class::ClassTable;
use crate::effects::ArgSlot;
use crate::effects::CallData;
use crate::effects::Effect;
use crate::effects::EffectData;
use crate::effects::EffectKind;
use crate::effects::EffectTable;
use crate::hasher::AHashMap;
use crate::hasher::AHashSet;
use crate::hasher::HashMapExt;
use crate::imports::ImportGraph;
use crate::module_safety::ForwardedParam;
use crate::module_safety::MutatedParam;
use crate::module_safety::MutationCandidate;
use crate::module_safety::MutationCandidateSite;
use crate::module_safety::ParamPosition;
use crate::project::dispatch::iter_callees;
use crate::project::info::ProjectInfo;
use crate::project::info::resolve_enclosing_module;
use crate::project::types::AnalysisMap;
use crate::traits::ModuleNameExt;

/// Whether a collection pass needs the parameter-forwarding records. Only the
/// incremental path does: whole-program resolves every forwarding locally.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CollectForwarding {
    Yes,
    No,
}

/// What one pass over the effects found for callees this library cannot resolve.
pub(super) struct CrossLibraryFacts {
    pub(super) candidates: AHashMap<ModuleName, Vec<MutationCandidate>>,
    pub(super) forwarded_params: AHashMap<ModuleName, Vec<ForwardedParam>>,
}

fn group_by_module<T>(flat: Vec<(ModuleName, T)>) -> AHashMap<ModuleName, Vec<T>> {
    let mut result: AHashMap<ModuleName, Vec<T>> = AHashMap::new();
    for (module, item) in flat {
        result.entry(module).or_default().push(item);
    }
    result
}

/// Per-scope context for mutation-candidate detection
#[derive(Clone, Copy)]
struct MutationCandidateScope<'a> {
    /// The enclosing module.
    module: ModuleName,
    /// Scope-local caller name (`None` at module scope)
    caller_function: Option<ModuleName>,
    /// Module's unresolved-import sets
    missing: Option<&'a AHashSet<ModuleName>>,
    ambiguous: Option<&'a AHashSet<ModuleName>>,
}

/// Whether `callee` (or one of its parents) is an unresolved import of the
/// enclosing module.
fn callee_is_unresolved(
    callee: &ModuleName,
    missing: Option<&AHashSet<ModuleName>>,
    ambiguous: Option<&AHashSet<ModuleName>>,
) -> bool {
    let is_unresolved = |name: &ModuleName| {
        missing.is_some_and(|m| m.contains(name)) || ambiguous.is_some_and(|a| a.contains(name))
    };
    is_unresolved(callee)
        || callee
            .iter_parents()
            .any(|(parent, _)| is_unresolved(&parent))
}

/// The candidate for one call passing an imported object to `callee`. Module
/// scope records the call as written, so the error can name it.
fn mutation_candidate(
    eff: &Effect,
    call_data: &CallData,
    caller_function: Option<ModuleName>,
    callee: ModuleName,
    arg_offset: usize,
) -> MutationCandidate {
    let site = match caller_function {
        Some(name) => MutationCandidateSite::Function { name },
        None => MutationCandidateSite::ModuleScope { call: eff.name },
    };
    MutationCandidate {
        callee,
        site,
        arg_offset,
        imported_args: call_data.imported_args().clone(),
        range: eff.range,
        from_main_guard: eff.from_main_guard,
    }
}

/// Per function, the set of parameters that are method-mutated either directly or transitively via
/// parameter forwarding.
///
/// TODO(T237092592):
/// - Cross-library forwarding is not tracked
/// - Unknown callee (no EffectTable entry) similarly drops the edge, inconsistent with other places
///   where unknown => unsafe
/// - `**kwargs` forwarding of a parameter is not tracked precisely; only named
///   keyword arguments are added to edges.
pub(super) fn compute_mutated_params(
    effect_table: &EffectTable,
    functions: &AHashMap<ModuleName, ModuleName>,
    classes: &ClassTable,
    analysis_map: &AnalysisMap,
) -> AHashMap<ModuleName, AHashSet<ModuleName>> {
    // Compute a fixpoint over the call graph. The "seed" is the base set of functions that directly
    // mutate their parameters, and an "edge" is a transitive mutation from callee->caller (reversed
    // from the call graph since mutations flow in the opposite direction to calls).

    // (function, mutated parameter)
    type SeedMutation = (ModuleName, ModuleName);
    // ((callee, target parameter), (caller, caller parameter))
    type Edge = ((ModuleName, ModuleName), SeedMutation);

    // Scan every scope's effects in parallel
    let (all_seeds, all_edges): (Vec<SeedMutation>, Vec<Edge>) = effect_table
        .par_iter()
        .fold(
            || (Vec::new(), Vec::new()),
            |(mut seeds, mut edges), (scope, effs)| {
                for eff in effs {
                    // Seed: parameters mutated directly in this function.
                    if eff.kind == EffectKind::ParamMethodCall {
                        seeds.push((*scope, eff.name));
                        continue;
                    }
                    // Parameter-forwarding edges recorded on call effects.
                    let Some(call_data) = eff.call_data() else {
                        continue;
                    };
                    let forwarded = call_data.forwarded_params();
                    if forwarded.is_empty() {
                        continue;
                    }
                    // A call may bind to several callees (a constructor's
                    // __init__/__new__); resolve each owner's parameter names once.
                    for (owner, arg_offset) in iter_callees(eff, classes) {
                        let owner_param_names = functions
                            .get(&owner)
                            .and_then(|module| analysis_map.get(module))
                            .and_then(|m| m.definitions.param_names.get(&owner));
                        for (slot, param) in forwarded {
                            match slot {
                                ArgSlot::Keyword(name) => {
                                    edges.push(((owner, *name), (*scope, *param)));
                                }
                                ArgSlot::Positional(idx) => {
                                    if let Some(target) = owner_param_names
                                        .and_then(|names| names.get(idx + arg_offset))
                                        .map(ModuleName::from_name)
                                    {
                                        edges.push(((owner, target), (*scope, *param)));
                                    }
                                }
                                ArgSlot::StarExpansion(start) => {
                                    // `*param` unpacks into the callee's positional
                                    // parameters from the star's position onward, so
                                    // forward to each parameter at or after that slot
                                    // (offset by the receiver). Leading positional args
                                    // before the star fill earlier params, which `*param`
                                    // cannot reach.
                                    if let Some(names) = owner_param_names {
                                        edges.extend(names.iter().skip(start + arg_offset).map(
                                            |name| {
                                                (
                                                    (owner, ModuleName::from_name(name)),
                                                    (*scope, *param),
                                                )
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                (seeds, edges)
            },
        )
        .reduce(
            || (Vec::new(), Vec::new()),
            |(mut seeds, mut edges), (s, e)| {
                seeds.extend(s);
                edges.extend(e);
                (seeds, edges)
            },
        );

    // Merge into `mutated` (seeds the fixpoint) and `dependents`, the reverse
    // index: (owner `g`, parameter `q`) -> callers whose parameter becomes mutated
    // once `q` is known mutated.
    // One entry per mutated scope (~3 seeds/scope)
    let mut mutated: AHashMap<ModuleName, AHashSet<ModuleName>> =
        AHashMap::with_capacity(all_seeds.len() / 3);
    let mut worklist: Vec<SeedMutation> = Vec::new();
    for (scope, param) in all_seeds {
        if mutated.entry(scope).or_default().insert(param) {
            worklist.push((scope, param));
        }
    }

    // One entry per mutated (owner, param), not per edge (~2 edges/key).
    let dependents: DashMap<(ModuleName, ModuleName), Vec<SeedMutation>> =
        DashMap::with_capacity(all_edges.len() / 2);
    all_edges.into_par_iter().for_each(|(key, dep)| {
        dependents.entry(key).or_default().push(dep);
    });

    // Fixpoint: when (g, q) becomes mutated, each caller forwarding a parameter
    // into q has that parameter become mutated too.
    while let Some((g, q)) = worklist.pop() {
        let Some(deps) = dependents.get(&(g, q)) else {
            continue;
        };
        for &(f, p) in deps.value() {
            if mutated.entry(f).or_default().insert(p) {
                worklist.push((f, p));
            }
        }
    }

    mutated
}

impl ProjectInfo {
    /// Collect the facts the reduce has to settle, grouped by the caller's
    /// module: mutation candidates and parameter forwarding this library
    /// cannot resolve.
    ///
    /// `forwarding` is `No` for whole-program, which resolves every forwarding
    /// locally and would only pay to discard the records.
    pub(super) fn collect_cross_library_facts(
        &self,
        import_graph: &ImportGraph,
        forwarding: CollectForwarding,
    ) -> CrossLibraryFacts {
        type Flat = (
            Vec<(ModuleName, MutationCandidate)>,
            Vec<(ModuleName, ForwardedParam)>,
        );
        let (mut flat_candidates, flat_forwarded): Flat = self
            .effect_table
            .par_iter()
            .fold(
                || (Vec::new(), Vec::new()),
                |(mut candidates, mut forwarded), (scope, effs)| {
                    let mut caller_scope = None;
                    let mut caller_defs = None;
                    for eff in effs {
                        let EffectData::Call(ref call_data) = eff.data else {
                            continue;
                        };
                        let wants_candidate = call_data.has_unsafe_args();
                        let wants_forwarding = forwarding == CollectForwarding::Yes
                            && call_data.has_forwarded_params();
                        if !wants_candidate && !wants_forwarding {
                            continue;
                        }
                        for (callee, arg_offset) in iter_callees(eff, &self.classes) {
                            // Already a known mutator here, so the map step applied it.
                            // A callee merely defined here is settled too when no
                            // forwarding is collected: only the incremental path can
                            // still learn it mutates, through a hop in another library.
                            if self.mutated_params.contains_key(&callee)
                                || (forwarding == CollectForwarding::No
                                    && self.functions.contains_key(&callee))
                            {
                                continue;
                            }
                            // Resolve and cache the scope's module, caller name, and the
                            // module's unresolved-import sets.
                            let Some(MutationCandidateScope {
                                module,
                                caller_function,
                                missing,
                                ambiguous,
                            }) = *caller_scope.get_or_insert_with(|| {
                                self.mutation_candidate_scope(scope, import_graph)
                            })
                            else {
                                // The module is unknown, so nothing here can be placed.
                                continue;
                            };
                            let local_callee = self.functions.contains_key(&callee);
                            if !local_callee && !callee_is_unresolved(&callee, missing, ambiguous) {
                                continue;
                            }
                            if wants_candidate && !local_callee {
                                candidates.push((
                                    module,
                                    mutation_candidate(
                                        eff,
                                        call_data,
                                        caller_function,
                                        callee,
                                        arg_offset,
                                    ),
                                ));
                            }
                            if !wants_forwarding {
                                continue;
                            }
                            // Module scope has no parameters to forward.
                            let Some(function) = caller_function else {
                                continue;
                            };
                            // Keyed on `module`, which is fixed for the scope, so
                            // this is resolved once per scope rather than per callee.
                            let defs = *caller_defs.get_or_insert_with(|| {
                                self.analysis_map.get(&module).map(|m| &m.definitions)
                            });
                            for (slot, param) in call_data.forwarded_params() {
                                // An unresolvable signature must not read as
                                // keyword-only, which would miss a positional match.
                                let position = match defs {
                                    Some(d) => d.classify_param(scope, param.as_str()),
                                    None => ParamPosition::Unresolved,
                                };
                                forwarded.push((
                                    module,
                                    ForwardedParam {
                                        function,
                                        param: *param,
                                        position,
                                        callee,
                                        slot: slot.clone(),
                                        arg_offset,
                                        from_main_guard: eff.from_main_guard,
                                    },
                                ));
                            }
                        }
                    }
                    (candidates, forwarded)
                },
            )
            .reduce(
                || (Vec::new(), Vec::new()),
                |(mut ca, mut fa), (mut cb, mut fb)| {
                    ca.append(&mut cb);
                    fa.append(&mut fb);
                    (ca, fa)
                },
            );

        // Reduce-time application is order-sensitive, so sort to keep the sequence
        // independent of how the parallel collection happened to interleave. The
        // forwarded records are not sorted: the fixpoint that reads them decides
        // against one frozen state, so their order carries nothing.
        flat_candidates.par_sort_unstable();

        CrossLibraryFacts {
            candidates: group_by_module(flat_candidates),
            forwarded_params: group_by_module(flat_forwarded),
        }
    }

    /// Resolve the enclosing module and scope-local caller name for `scope`, together with the
    /// module's unresolved-import sets (fetched once per scope).
    /// Returns `None` when the module can't be located.
    fn mutation_candidate_scope<'a>(
        &self,
        scope: &ModuleName,
        import_graph: &'a ImportGraph,
    ) -> Option<MutationCandidateScope<'a>> {
        // A module-scope effect has no enclosing function; anything else is
        // named relative to the module that contains it.
        let (module, caller_function) = if self.analysis_map.contains_key(scope) {
            (*scope, None)
        } else {
            let (module, local) =
                resolve_enclosing_module(scope, |name| self.analysis_map.contains_key(name))?;
            (module, Some(ModuleName::from_str(local)))
        };
        let missing = import_graph.get_missing_imports(&module);
        let ambiguous = import_graph.get_ambiguous_imports(&module);
        Some(MutationCandidateScope {
            module,
            caller_function,
            missing,
            ambiguous,
        })
    }

    /// `func`'s (transitively) mutated parameters, each classified against its own
    /// signature. A signature that can't be found is `Unresolved`, never keyword-only.
    fn classified_mutated_params<'a>(
        &'a self,
        func: &ModuleName,
    ) -> Option<impl Iterator<Item = (&'a ModuleName, ParamPosition)> + 'a> {
        let params = self.mutated_params.get(func)?;
        let module = self.functions.get(func).unwrap_or(func);
        let defs = self.analysis_map.get(module).map(|m| &m.definitions);
        let func = *func;
        Some(params.iter().map(move |param| {
            let position = defs.map_or(ParamPosition::Unresolved, |d| {
                d.classify_param(&func, param.as_str())
            });
            (param, position)
        }))
    }

    /// Resolve a function's mutated parameters to positional indices using the
    /// callee's definition table.
    pub(super) fn resolve_cached_mutated_params_for(
        &self,
        func: &ModuleName,
    ) -> Option<Vec<MutatedParam>> {
        self.classified_mutated_params(func).map(|params| {
            params
                .map(|(&name, position)| MutatedParam { name, position })
                .collect()
        })
    }

    /// Whether `call_data` passes an imported variable into a (transitively)
    /// mutated parameter of `callee`, with explicit args starting at `arg_offset`.
    fn callee_mutates_imported_arg(
        &self,
        call_data: &CallData,
        callee: &ModuleName,
        arg_offset: usize,
    ) -> bool {
        self.classified_mutated_params(callee)
            .is_some_and(|params| {
                call_data.imported_args().hits_any_param(
                    params.map(|(param, position)| (param.as_str(), position)),
                    arg_offset,
                )
            })
    }

    /// Whether `call_effect` passes an imported variable to a parameter the callee
    /// (transitively) mutates, i.e. running this call mutates imported state.
    pub(super) fn call_mutates_imported_arg(&self, call_effect: &Effect) -> bool {
        let Some(call_data) = call_effect.call_data() else {
            return false;
        };
        if !call_data.has_unsafe_args() {
            return false;
        }
        iter_callees(call_effect, &self.classes)
            .any(|(callee, offset)| self.callee_mutates_imported_arg(call_data, &callee, offset))
    }

    /// Whether `call_effect` is a call that passes an imported variable as an
    /// argument. Such a call must be deferred to the reduce step when its callee
    /// is unresolved in this library (the callee may mutate the argument).
    pub(super) fn defers_cross_library_mutation(&self, call_effect: &Effect) -> bool {
        call_effect
            .call_data()
            .is_some_and(|data| data.has_unsafe_args())
    }
}
