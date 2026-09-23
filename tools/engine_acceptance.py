#!/usr/bin/env python3
"""What the engine has actually accepted, as data — and every sentence about it, rendered.

**Why this is a module and not a string.** `tools/mod_build.py` writes an engine-acceptance caveat
into every `build.json`, deliberately, so a build carries its own limits with it. That caveat was a
hand-written sentence, and it went stale: until 2026-09-18 it said no rewritten `pic.mpq` had ever
faced the engine and that the compression choice for one was Inferred, both of which this
repository's own roadmap had already refuted. The first repair was a test that grepped the sentence
for five phrases. Three review rounds then broke that test three times, each time through a gap the
previous repair had not closed:

1. the phrases were all present in a caveat whose `NOT established` had been changed to `ALSO
   established`;
2. with polarity asserted, `have none of them` changed to `have every one of them`;
3. with the negative clause's contents asserted, a new sentence appended after it —
   "In fact, the engine accepted a size-changing edit, an added member, and the full ByteRun1
   encoder" — left every assertion green.

That is not three bugs, it is one: **a finite set of assertions about prose cannot constrain the
open set of sentences prose can be.** So the prose stops being the artifact. The facts are the
artifact, the sentences are rendered from them, and the tests assert the facts.

**What this makes unrepresentable.** A run records what it *was* — how many members, replaced or
added, length-preserving or size-changing — and the limits that follow are **derived**, not typed.
Claiming the engine accepted a size-changing edit means setting `edit_kind` to `SIZE_CHANGING`,
which changes the rendered roadmap paragraph, which no longer matches `docs/roadmap.md`, which
fails.

**Every field that reaches the sentence carries a rule, which took two passes to get right.** The
first version of this module said "there is nowhere to append a sentence, because no sentence is
stored", and that was false: `mechanism`, `observation` and `storage_class` were all free text,
and a reviewer shipped the historical bypass verbatim through two of them with the whole suite
green. `mechanism` and `storage_class` are enums now; `observation` is the one sentence left and
is doubly constrained — a number it carries has to be the run's own count or date, and the
sentence has to appear in a marked region of the documentation. A run that derives no limits at
all is refused outright, because it used to render "Not established: ." and mean the opposite of
what this module is for.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from enum import Enum

DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
NUMBER = re.compile(r"\d[\d,]*")


class Disposition(Enum):
    """Whether the member the engine read was already in the archive."""

    REPLACED = "replaced rather than added"
    ADDED = "added rather than replaced"


class EditKind(Enum):
    """Whether the edit changed the member's size, which is the thing the writer risks."""

    LENGTH_PRESERVING = "length-preserving"
    SIZE_CHANGING = "size-changing"


class Mechanism(Enum):
    """What made the edit the engine then read.

    An enum rather than a string because this renders into the sentence, and a field that renders
    into a claim and accepts any text is the defect this module exists to remove: appending
    " The engine ALSO accepted a size-changing edit..." to a free-text mechanism shipped that
    sentence in every `build.json` with the whole suite green.
    """

    PBM_PATCH = "tools/pbm_patch.py"
    PIPELINE_WRITER = "the mod pipeline's own writer"
    WAVE_IMPORTER = "the WAVE importer"
    IMP_ENCODER = "the IMP pixel encoder"
    PBM_ENCODER = "the ByteRun1 PBM encoder"


class StorageClass(Enum):
    """The MPQ storage class a run covered, and what that settles.

    Same reasoning as `Mechanism`. The text is fixed here; the only choice a record makes is which
    of these applies.
    """

    IMPLODE_PROVED = (
        "The member carried flags 0x80010100 (EXISTS | ENCRYPTED | IMPLODE), which is the only "
        "storage class any run has covered."
    )
    STORED_PROVED = (
        "The members carried flags 0x80010000 (EXISTS | STORED), a storage class no earlier run "
        "had covered."
    )
    IMPLODE_BY_CENSUS = (
        "The compression choice is not Inferred: all 1,071 baseline members carry flags "
        "0x80010100, the same storage class the gs.mpq run proved."
    )


@dataclass(frozen=True)
class EngineRun:
    """One attended run in which the engine read something this pipeline wrote."""

    date: str
    members: int
    disposition: Disposition
    edit_kind: EditKind
    mechanism: Mechanism
    observation: str

    def __post_init__(self) -> None:
        if not DATE.match(self.date):
            raise ValueError(f"an engine run needs an ISO date, not {self.date!r}")
        if self.members < 1:
            raise ValueError("an engine run that read no member is not an engine run")
        if not isinstance(self.mechanism, Mechanism):
            raise ValueError("a mechanism is one of the recorded ones, not free text")
        if not self.observation.strip():
            raise ValueError("an engine run has to say what was observed")
        # `observation` is the one free-text field left, and free text is where a widened claim
        # hides: a reviewer widened it to "Each of the 1,071 members was re-encoded and accepted",
        # which every structural assertion passed. Quantities are fields, so a number that is
        # neither the member count nor part of the date cannot appear in the sentence.
        allowed = {str(self.members)} | set(self.date.split("-"))
        for match in NUMBER.finditer(self.observation):
            # A hexadecimal flag word is a name, not a quantity: `0x80010100` would otherwise make
            # a truthful observation about the storage class unrepresentable.
            if match.start() >= 2 and self.observation[match.start() - 2 : match.start()] == "0x":
                continue
            number = match.group()
            if number.replace(",", "") not in allowed and number not in allowed:
                raise ValueError(
                    f"{number!r} in an observation is a quantity this run does not record; "
                    "counts belong in fields, not in the sentence"
                )

    @property
    def derived_limits(self) -> tuple[str, ...]:
        """The limits that follow from what the run was, rather than from anyone's judgement.

        This is the whole point of the module. `not_established` cannot be quietly emptied while
        `run` still describes a single replaced, length-preserving member, because these come from
        the run.
        """
        limits = []
        if self.edit_kind is EditKind.LENGTH_PRESERVING:
            limits.append("an edit that changes a member's size")
        if self.disposition is Disposition.REPLACED:
            limits.append("a member added to an archive rather than replaced")
        if self.members == 1:
            limits.append("a second member of the same archive in one build")
        return tuple(limits)


@dataclass(frozen=True)
class ArchiveAcceptance:
    """What is established for one archive, and what explicitly is not."""

    archive: str
    runs: tuple[EngineRun, ...] = ()
    storage_class: StorageClass | None = None
    also_untested: tuple[str, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        if not self.archive.endswith(".mpq"):
            raise ValueError(f"{self.archive!r} is not an archive name")
        if not self.runs and self.also_untested == ():
            raise ValueError(
                f"{self.archive} has no engine run and names no limit; an archive nothing is "
                "known about still has to say so"
            )
        if not self.not_established:
            # Reachable, and it was: a run recorded as size-changing, added and multi-member
            # derives no limits at all, and `summary()` then rendered "Not established: ." -- a
            # build claiming the engine had accepted everything. One attended run never
            # establishes a whole archive, so an empty list is a defect rather than a boast.
            raise ValueError(
                f"{self.archive} claims an engine run with nothing left unestablished; no run "
                "covers a whole archive, so name what it did not cover"
            )

    @property
    def not_established(self) -> tuple[str, ...]:
        """What no run has shown, across every run this archive has had.

        A limit stands only while EVERY run exhibits it. One run that changed a member's size
        settles that question for this archive however many earlier runs did not -- and the
        intersection is what makes that automatic rather than a judgement someone has to remember
        to revisit. Recording a second run and leaving the first run's limits standing is exactly
        how this file came to publish three limits that runs had already refuted.
        """
        if not self.runs:
            return self.also_untested
        common = set(self.runs[0].derived_limits)
        for run in self.runs[1:]:
            common &= set(run.derived_limits)
        ordered: list[str] = []
        for run in self.runs:
            for limit in run.derived_limits:
                if limit in common and limit not in ordered:
                    ordered.append(limit)
        return tuple(ordered) + self.also_untested

    def summary(self) -> str:
        """The sentence `build.json` carries. Rendered; never edited in place."""
        limits = "; ".join(self.not_established)
        if not self.runs:
            return (
                f"Never tested. No {self.archive} this pipeline wrote has been put in front of "
                f"the engine. Not established: {limits}."
            )
        times = "once" if len(self.runs) == 1 else f"{len(self.runs)} times"
        sentences = []
        for run in self.runs:
            member_word = "member" if run.members == 1 else "members"
            sentences.append(
                f"{run.date}: {run.members} {member_word} of {self.archive}, "
                f"{run.disposition.value}, with a {run.edit_kind.value} edit made by "
                f"{run.mechanism.value}. {run.observation}"
            )
        established = f"Observed {times}. " + " ".join(sentences)
        storage = f" {self.storage_class.value}" if self.storage_class else ""
        return f"{established} Not established: {limits}.{storage}"

    def as_json(self) -> dict:
        """The same facts as structure, so a reader does not have to parse the sentence."""
        record: dict = {
            "summary": self.summary(),
            "not_established": list(self.not_established),
        }
        record["established"] = [
            {
                "date": run.date,
                "members": run.members,
                "disposition": run.disposition.name.lower(),
                "edit_kind": run.edit_kind.name.lower(),
                "mechanism": run.mechanism.value,
                "observation": run.observation,
            }
            for run in self.runs
        ] or None
        if self.storage_class:
            record["storage_class"] = self.storage_class.value
        return record


# The measured record. Every sentence in `build.json` and the one in `docs/roadmap.md` is rendered
# from here, and `tests/test_mod_pipeline.py` asserts the roadmap still contains what this renders.
ACCEPTANCE: dict[str, ArchiveAcceptance] = {
    "gs.mpq": ArchiveAcceptance(
        archive="gs.mpq",
        runs=(
            EngineRun(
            date="2026-09-16",
            members=1,
            disposition=Disposition.REPLACED,
            edit_kind=EditKind.LENGTH_PRESERVING,
            mechanism=Mechanism.PIPELINE_WRITER,
            observation=(
                "The attended 2026-09-16 round trip of an MPQ_FILE_IMPLODE member "
                "of gs.mpq."
            ),
            ),
            EngineRun(
                date="2026-09-19",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.SIZE_CHANGING,
                mechanism=Mechanism.PIPELINE_WRITER,
                observation=(
                    "The cheat-keys ladder flipped a single token in a member of gs.mpq, which "
                    "made the member shorter, and the debug hotkey tier it gates was then "
                    "exercised in gameplay."
                ),
            ),
        ),
        storage_class=StorageClass.IMPLODE_PROVED,
        also_untested=("any flag combination other than 0x80010100",),
    ),
    "pic.mpq": ArchiveAcceptance(
        archive="pic.mpq",
        runs=(EngineRun(
            date="2026-09-18",
            members=1,
            disposition=Disposition.REPLACED,
            edit_kind=EditKind.LENGTH_PRESERVING,
            mechanism=Mechanism.PBM_PATCH,
            observation=(
                "The engine read an archive this pipeline built from pic.mpq, and a human read "
                "the change off the screen."
            ),
        ),
            EngineRun(
                date="2026-09-20",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.SIZE_CHANGING,
                mechanism=Mechanism.PBM_ENCODER,
                observation=(
                    "The acceptance ladder re-encoded the main-menu backdrop with the ByteRun1 "
                    "encoder, once with the shipped pixels and once with a filled rectangle; the "
                    "member shrank, its TINY thumbnail was dropped, and the observer read the "
                    "straight-edged rectangle off the screen with the surrounding art intact."
                ),
            ),
        ),
        storage_class=StorageClass.IMPLODE_BY_CENSUS,
        also_untested=(
            "a TINY thumbnail regenerated rather than dropped, which needs a downscaler this "
            "repo does not have",
        ),
    ),
    "imp.mpq": ArchiveAcceptance(
        archive="imp.mpq",
        runs=(
            EngineRun(
                date="2026-09-17",
                members=2,
                disposition=Disposition.ADDED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.IMP_ENCODER,
                observation=(
                    "The sprite-ladder probe added two members no shipped archive holds and a "
                    "script asked for one of them BY NAME; the engine registered a terrain "
                    "sprite type from it and painted its pixels, which is how the stored palette "
                    "order was measured -- the authored entries exist in no other member. (The "
                    "order it gave was wrong: the capture reader swapped red and green, as the "
                    "research log's later correction records.)"
                ),
            ),
            EngineRun(
                date="2026-09-20",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.IMP_ENCODER,
                observation=(
                    "The repack control: the cursor member was re-emitted by the IMP encoder "
                    "byte for byte, the archive was repacked under the recovered names, and the "
                    "observer reported the shipped pointer unchanged."
                ),
            ),
            EngineRun(
                date="2026-09-20",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.IMP_ENCODER,
                observation=(
                    "Disjoint palette-index swaps repainted the cursor frame at unchanged "
                    "payload length, and the observer reported the pointer had changed colour."
                ),
            ),
            EngineRun(
                date="2026-09-20",
                members=2,
                disposition=Disposition.ADDED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.IMP_ENCODER,
                observation=(
                    "A member no shipped archive holds was added alongside the repainted cursor; "
                    "the archive grew, and the observer reported the repainted pointer still "
                    "drawn, so every original member survived the hash and block tables growing."
                ),
            ),
        ),
        storage_class=StorageClass.IMPLODE_PROVED,
        also_untested=(
            "an added member reaching the engine through the MOD PIPELINE and being read; the "
            "2026-09-17 probe added its member with mpq_replace, and rung 5's pipeline-built "
            "member was never asked for by anything in the game",
        ),
    ),
    "sndfx.mpq": ArchiveAcceptance(
        archive="sndfx.mpq",
        runs=(
            EngineRun(
                date="2026-09-19",
                members=2,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.WAVE_IMPORTER,
                observation=(
                    "The acceptance ladder wrote a tone into both audio archives and the listener named which "
                    "archive the engine had opened; exchanging the two tones flipped the report."
                ),
            ),
        ),
        storage_class=StorageClass.STORED_PROVED,
    ),
    "special.mpq": ArchiveAcceptance(
        archive="special.mpq",
        runs=(
            EngineRun(
                date="2026-09-19",
                members=2,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.WAVE_IMPORTER,
                observation=(
                    "The acceptance ladder wrote a tone into both audio archives and the listener named which "
                    "archive the engine had opened; exchanging the two tones flipped the report."
                ),
            ),
        ),
        storage_class=StorageClass.STORED_PROVED,
    ),
}


def build_metadata() -> dict:
    """What `build.json` records: the sentence and the structure behind it, per archive."""
    return {name: acceptance.as_json() for name, acceptance in sorted(ACCEPTANCE.items())}


MARKER = "engine-acceptance"


def roadmap_region(archive: str) -> tuple[str, str]:
    """The HTML comments bracketing this archive's paragraph in `docs/roadmap.md`."""
    return f"<!-- {MARKER}:{archive} -->", f"<!-- /{MARKER}:{archive} -->"


def roadmap_paragraph(archive: str) -> str:
    """The `Not established.` paragraph in `docs/roadmap.md`, rendered from the same facts.

    The doc is checked against this rather than the other way round, so a claim can only widen in
    both places at once and only by editing the data. It carries the observation too, so that
    sentence -- the one free-text field left -- is pinned to a **marked region** of the document
    rather than to "appears somewhere in 1,400 lines". The earlier containment rule was blind to
    polarity and to place: `docs/build-pipeline.md` quotes a refuted claim in order to refute it,
    and any sub-span of that quotation would have passed.
    """
    acceptance = ACCEPTANCE[archive]
    if not acceptance.runs:
        return (
            f"**Not established.** Nothing: no `{archive}` this pipeline wrote has been put in "
            "front of the engine."
        )
    untested = [name for name, other in sorted(ACCEPTANCE.items()) if not other.runs]
    remainder = ""
    if len(untested) == 1:
        remainder = f" `{untested[0]}` remains untested."
    elif untested:
        listed = ", ".join(f"`{name}`" for name in untested[:-1])
        remainder = f" {listed} and `{untested[-1]}` remain untested."
    storage = f" {acceptance.storage_class.value}" if acceptance.storage_class else ""
    runs = " ".join(
        f"{run.members} {'member' if run.members == 1 else 'members'} of one `{archive}`, "
        f"{run.disposition.value}, with a {run.edit_kind.value} edit made by "
        f"{run.mechanism.value}, {run.date}. {run.observation}"
        for run in acceptance.runs
    )
    return (
        f"**Not established.** {runs} "
        + " ".join(f"The engine has not been shown {limit}." for limit in acceptance.not_established)
        + storage
        + remainder
    )
