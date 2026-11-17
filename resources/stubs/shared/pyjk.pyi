# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# Construction captures a name and deferred native factory; evaluation reads JK.
# APIs not verified below retain unknown effects, including other constructors.

from contextlib import AbstractContextManager
from typing import List, Mapping, Optional, Set, Tuple, Union

class BooleanKnob:
    name: str

    # Keep construction unknown until implicit truth-conversion effects are modeled.
    def __init__(self, name: str) -> None:
        ...

    def __bool__(self) -> bool:
        unsafe()

    def __call__(self, hashval: Optional[Union[str, int]]=..., switchval: Optional[str]=...) -> bool:
        unsafe()

class IntegerKnob:
    name: str

    def __init__(self, name: str) -> None:
        no_effects()

    def __call__(self, switchval: Optional[str]=...) -> int:
        unsafe()

class PyPatchJustKnobs:

    def __init__(self, *args: object, **kwargs: object) -> None:
        ...

    def patch(self, name: str, value: Union[bool, int], switches: Optional[Union[Mapping[str, int], Mapping[str, bool]]]=...) -> AbstractContextManager:
        ...

    def patch_bulk(self, patches: Mapping[str, Union[Union[bool, Tuple[bool, Mapping[str, bool]]], Union[int, Tuple[int, Mapping[str, int]]]]]) -> None:
        ...

    def patch_all(self) -> None:
        ...

def check(name: str, hashval: Optional[Union[str, int]]=..., switchval: Optional[str]=...) -> bool:
    ...

def getval(name: str, switchval: Optional[str]=...) -> int:
    ...

def assert_legacy_knob_allowed(name: str) -> None:
    ...

def ready() -> None:
    ...

def preload_knobsets_for_forking(knobset_names: Set[str]) -> None:
    ...

def apply_preloaded_knobsets_after_forking() -> None:
    ...

def get_with_default_DO_NOT_USE(name: str, default: int) -> int:
    ...

def get_with_default_MAYBE_NONE_DO_NOT_USE_SEE_YOU_IN_SEV_REVIEW(name: str, default: object) -> object:
    ...

def list_all_knobs_EXPENSIVE() -> List[str]:
    ...

def list_all_knobs_with_test_values() -> Mapping[str, Union[int, bool]]:
    ...

def list_knobs_modified_since(since: int) -> List[Tuple[str, int]]:
    ...

def set_tracked_knobsets(knobset_names: Set[str]) -> None:
    ...

def reset_tracked_knobsets() -> None:
    ...

def get_justknobs_read_in_current_request() -> Optional[Set[str]]:
    ...

class TrackableBooleanKnob:
    name: str

    def __init__(self, name: str) -> None:
        ...

    def __call__(self, hashval: Optional[Union[str, int]]=..., switchval: Optional[str]=...) -> bool:
        ...

class TrackableIntegerKnob:
    name: str

    def __init__(self, name: str) -> None:
        ...

    def __call__(self, switchval: Optional[str]=...) -> int:
        ...

class InstagramHelpers:

    def __init__(self, *args: object, **kwargs: object) -> None:
        ...

    @staticmethod
    def preload_configs(configs: Set[str]) -> None:
        ...
