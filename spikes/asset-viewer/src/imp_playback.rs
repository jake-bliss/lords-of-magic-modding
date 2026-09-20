//! Playing an `.imp` the way the engine plays it: `AnimRules` applied to a decoded sprite.
//!
//! `imp.rs` decodes the container and `imp_anim.rs` recovers the rules out of `lomse.exe`. Until
//! this module existed the two never met: the recovery ran, the survey printed it, and the SDL
//! viewer walked frames with a rule of its own invention -- "advance one index inside the current
//! facing, wrap at the end" -- which is not what any of the five cycle modes do and which has no
//! notion of a direction at all.
//!
//! What the rules determine, and the whole of what they determine:
//!
//! - **order** -- the cycle mode decides whether a run of frames loops, holds its last frame, or
//!   reflects (`cycle_length`, `frame_for_cycle_index`);
//! - **reflection** -- mode 4 is a ping-pong of `2N-1` steps rather than a forward run of `N`;
//! - **facings** -- the mirror bit makes a sequence cover `2N-2` directions with `N` stored
//!   facings, the surplus drawn horizontally flipped (`direction_count`,
//!   `facing_for_direction`).
//!
//! **They do not determine an interval.** No field of an `.imp` is read by the engine as a
//! duration, delay, rate or tick count -- see `docs/imp-format.md`, "Timing: there is none in the
//! *file*", **Observed in a local binary**. That does not make the interval unknowable, only
//! absent from the asset: the engine's tick period is a field of the screen-mode object
//! (`+0x228AC`, `idiv` at 0x004822FF) that `gs/modeinfo.gs` sets to 66 or 121 ms. The asset says
//! which frame comes next; the screen mode says how long to wait. This module answers the first
//! question and takes no position on the second.
//!
//! Nothing here reads a module constant. Every rule is taken from the [`AnimRules`] the caller
//! recovered from a binary, for the reason `imp_anim` gives: a constant that both the
//! implementation and its test read is self-consistent whatever it holds.

use std::fmt;

use crate::imp::ImpSprite;
use crate::imp_anim::{
    AnimRules, CycleEnd, cycle_length, cycle_mode, direction_count, facing_for_direction,
    frame_for_cycle_index,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackError(String);

impl PlaybackError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PlaybackError {}

/// Where a playhead is: an action, a direction, and a position inside the cycle.
///
/// The engine's own state is the same three values -- `Imp::SetAction` caches the sequence record,
/// `Imp::SetFacing` sets the direction, and the frame index lives at
/// [`crate::imp_anim::FRAME_INDEX_FIELD`]. `position` is that frame index: a position in the
/// *cycle*, which for a ping-pong runs past the stored frame count and folds back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playhead {
    /// Index into `ImpSprite::sequences`; the engine's *action*.
    pub sequence: usize,
    /// The engine's direction index, `0..`[`usable_directions`].
    pub direction: usize,
    /// The cycle position, `0..`[`Resolved::cycle_length`]. Not a frame index.
    pub position: usize,
}

/// What a playhead resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Index into `ImpSprite::facings`.
    pub facing: usize,
    /// Index into `ImpSprite::frames`.
    pub frame: usize,
    /// Whether the engine draws this direction horizontally flipped.
    pub flipped: bool,
    /// The sequence's cycle mode, masked as the engine masks it.
    pub mode: u8,
    /// What the engine does when the position runs off the end of this cycle.
    pub end: CycleEnd,
    /// How many positions the cycle has: `2N-1` for a ping-pong, `N` otherwise.
    pub cycle_length: usize,
    /// The stored frame count of the facing this direction resolves to.
    pub facing_frames: usize,
}

/// How many directions the sequence advertises, straight from the engine's `Imp::DirectionCount`.
pub fn advertised_directions(
    rules: &AnimRules,
    sprite: &ImpSprite,
    sequence: usize,
) -> Result<usize, PlaybackError> {
    let record = sequence_record(sprite, sequence)?;
    Ok(direction_count(
        rules.mirror_bit,
        &record.metadata,
        record.facing_count,
    ))
}

/// How many directions a caller may actually ask for.
///
/// This is [`advertised_directions`] except for one case the engine tolerates and the arithmetic
/// does not: a single-facing sequence with the mirror bit set advertises `2 x 1 - 2 = 0`
/// directions. 991 of the shipped sequences are in that state (**Observed in the corpus**,
/// `docs/imp-format.md`). It is inert rather than broken -- the only direction ever asked for is
/// 0, and 0 is below the facing count, so `Imp::GetFacing` returns facing 0 unmirrored before the
/// fold is reached. A viewer that iterated `0..advertised` would show those 991 sequences nothing
/// at all, so it iterates this instead.
///
/// A sequence with no facings has no directions, and that is not the same case.
pub fn usable_directions(
    rules: &AnimRules,
    sprite: &ImpSprite,
    sequence: usize,
) -> Result<usize, PlaybackError> {
    let record = sequence_record(sprite, sequence)?;
    if record.facing_count == 0 {
        return Ok(0);
    }
    Ok(advertised_directions(rules, sprite, sequence)?.max(1))
}

/// Resolve a playhead to a frame, reproducing `Imp::GetFrame`.
pub fn resolve(
    rules: &AnimRules,
    sprite: &ImpSprite,
    head: Playhead,
) -> Result<Resolved, PlaybackError> {
    let record = sequence_record(sprite, head.sequence)?;
    let mode = cycle_mode(&record.metadata);
    // The dispatch bound is the processor's own `ja`, so a mode past it is a sequence the engine
    // does not clamp: the index runs on and `Imp::GetFrame`'s range check fails. Refusing here
    // reports that rather than inventing an ending for it.
    let end = rules
        .cycle_modes
        .modes
        .iter()
        .find(|entry| entry.mode == mode)
        .map(|entry| entry.end)
        .ok_or_else(|| {
            PlaybackError::new(format!(
                "sequence {} uses cycle mode {mode}, which the {}-entry dispatch at {:#010x} does \
                 not cover; the engine would not clamp it",
                head.sequence,
                rules.cycle_modes.modes.len(),
                rules.cycle_modes.table_address
            ))
        })?;
    let (relative_facing, flipped) = facing_for_direction(
        rules.mirror_bit,
        &record.metadata,
        record.facing_count,
        head.direction,
    )
    .ok_or_else(|| {
        PlaybackError::new(format!(
            "sequence {} has no facing for direction {}; it stores {} facings",
            head.sequence, head.direction, record.facing_count
        ))
    })?;
    let facing_index = record.first_facing + relative_facing;
    let facing = sprite.facings.get(facing_index).ok_or_else(|| {
        PlaybackError::new(format!(
            "sequence {} names facing {facing_index}, past the {} the sprite decodes",
            head.sequence,
            sprite.facings.len()
        ))
    })?;
    // `Imp::CycleLength` reads the frame count off the *facing* record (`mov cx,[eax+2]` at
    // 0x0049D915), not off the sequence -- the sequence's count is the sum over its facings.
    let length = cycle_length(rules.ping_pong_mode, mode, facing.frame_count);
    let offset = frame_for_cycle_index(
        rules.ping_pong_mode,
        mode,
        facing.frame_count,
        head.position,
    )
    .ok_or_else(|| {
        PlaybackError::new(format!(
            "cycle position {} is past the {length}-step cycle of sequence {} direction {}",
            head.position, head.sequence, head.direction
        ))
    })?;
    let frame = facing.first_frame + offset;
    if frame >= sprite.frames.len() {
        return Err(PlaybackError::new(format!(
            "sequence {} direction {} position {} names frame {frame}, past the {} the sprite \
             decodes",
            head.sequence,
            head.direction,
            head.position,
            sprite.frames.len()
        )));
    }
    Ok(Resolved {
        facing: facing_index,
        frame,
        flipped,
        mode,
        end,
        cycle_length: length,
        facing_frames: facing.frame_count,
    })
}

/// One tick, reproducing `Imp::Advance`.
///
/// Returns the new playhead and whether the cycle **ended** on this tick. Both endings report
/// completion -- `0x0049DA0C` stores 1 into the local `Advance` returns at `0x0049DA4E` -- so a
/// looping cycle reports `true` every time it wraps. What the engine's caller does with that is
/// switch the action; this module has no caller to switch to and says so instead of guessing.
pub fn advance(
    rules: &AnimRules,
    sprite: &ImpSprite,
    head: Playhead,
) -> Result<(Playhead, bool), PlaybackError> {
    let resolved = resolve(rules, sprite, head)?;
    let next = head.position + 1;
    if next < resolved.cycle_length {
        return Ok((
            Playhead {
                position: next,
                ..head
            },
            false,
        ));
    }
    let position = match resolved.end {
        CycleEnd::WrapToStart => 0,
        CycleEnd::HoldLastFrame => resolved.cycle_length.saturating_sub(1),
        CycleEnd::Unclassified => {
            return Err(PlaybackError::new(format!(
                "cycle mode {} dispatches to {:#010x}, which this classifier does not recognise; \
                 what happens at the end of the cycle is not established",
                resolved.mode,
                rules
                    .cycle_modes
                    .modes
                    .iter()
                    .find(|entry| entry.mode == resolved.mode)
                    .map_or(0, |entry| entry.target)
            )));
        }
    };
    Ok((Playhead { position, ..head }, true))
}

/// Navigation helpers for an interactive viewer.
///
/// These are not engine behaviour and do not claim to be. The engine plays forward and switches
/// action when a cycle reports completion; a viewer scrubs, turns on the spot, and has to skip
/// records whose frames decode to nothing. What they *do* preserve is that every playhead they
/// produce is a position the engine's own fold resolves -- the order and the facing choice stay
/// [`resolve`]'s, and only which of those positions gets shown is the viewer's business.
///
/// `visible` answers whether a frame index has pixels worth putting on screen. The library
/// cannot answer that itself without decoding, so the caller does.
pub struct Navigator<'a> {
    pub rules: &'a AnimRules,
    pub sprite: &'a ImpSprite,
    pub visible: &'a dyn Fn(usize) -> bool,
}

impl Navigator<'_> {
    fn shows(&self, head: Playhead) -> bool {
        resolve(self.rules, self.sprite, head).is_ok_and(|resolved| (self.visible)(resolved.frame))
    }

    /// The first playhead at or after `frame` that shows something.
    ///
    /// `frame` is a global frame index, the argument `--view-imp` takes. The frame it names is
    /// preferred exactly: a stored facing is reached by the direction of the same relative index,
    /// which is the unmirrored half of the fold, and the position inside it is the offset the
    /// decoder reports. If that frame decodes to nothing the search falls forward through the
    /// sprite in sequence, direction and position order.
    pub fn playable_at(&self, frame: usize) -> Result<Playhead, PlaybackError> {
        if self.sprite.sequences.is_empty() {
            return Err(PlaybackError::new("the sprite has no sequences"));
        }
        let mut start = 0;
        if let Ok((sequence, facing, offset)) = self.sprite.frame_location(frame) {
            start = sequence;
            let direction = facing - self.sprite.sequences[sequence].first_facing;
            if direction < usable_directions(self.rules, self.sprite, sequence)? {
                let requested = Playhead {
                    sequence,
                    direction,
                    position: offset,
                };
                if self.shows(requested) {
                    return Ok(requested);
                }
            }
        }
        for step in 0..self.sprite.sequences.len() {
            let sequence = (start + step) % self.sprite.sequences.len();
            let directions = usable_directions(self.rules, self.sprite, sequence)?;
            for direction in 0..directions {
                let head = Playhead {
                    sequence,
                    direction,
                    position: 0,
                };
                if let Some(found) = self.first_visible_position(head) {
                    return Ok(found);
                }
            }
        }
        Err(PlaybackError::new(
            "the sprite has no sequence, direction and position that decodes to any pixels",
        ))
    }

    fn first_visible_position(&self, head: Playhead) -> Option<Playhead> {
        let length = resolve(self.rules, self.sprite, head).ok()?.cycle_length;
        (0..length)
            .map(|position| Playhead { position, ..head })
            .find(|candidate| self.shows(*candidate))
    }

    /// Move the playhead along the cycle by `delta` positions, wrapping, skipping what does not
    /// show. Scrubbing backwards is the viewer's own affordance: the engine only ever advances.
    pub fn scrub(&self, head: Playhead, delta: isize) -> Result<Playhead, PlaybackError> {
        let length = resolve(self.rules, self.sprite, head)?.cycle_length;
        if length == 0 {
            return Err(PlaybackError::new("the cycle has no positions"));
        }
        for step in 1..=length {
            let position = (head.position as isize + delta * step as isize)
                .rem_euclid(length as isize) as usize;
            let candidate = Playhead { position, ..head };
            if self.shows(candidate) {
                return Ok(candidate);
            }
        }
        Err(PlaybackError::new(
            "no position in this cycle decodes to any pixels",
        ))
    }

    /// Turn to another direction of the same action, wrapping over the usable directions.
    pub fn turn(&self, head: Playhead, delta: isize) -> Result<Playhead, PlaybackError> {
        let directions = usable_directions(self.rules, self.sprite, head.sequence)?;
        if directions == 0 {
            return Err(PlaybackError::new("the sequence has no directions"));
        }
        for step in 1..=directions {
            let direction = (head.direction as isize + delta * step as isize)
                .rem_euclid(directions as isize) as usize;
            let candidate = Playhead {
                direction,
                position: 0,
                ..head
            };
            if let Some(found) = self.first_visible_position(candidate) {
                return Ok(found);
            }
        }
        Err(PlaybackError::new(
            "no direction of this sequence decodes to any pixels",
        ))
    }

    /// Move to another action, wrapping, skipping sequences that show nothing.
    pub fn change_action(&self, head: Playhead, delta: isize) -> Result<Playhead, PlaybackError> {
        let count = self.sprite.sequences.len();
        if count == 0 {
            return Err(PlaybackError::new("the sprite has no sequences"));
        }
        for step in 1..=count {
            let sequence = (head.sequence as isize + delta * step as isize)
                .rem_euclid(count as isize) as usize;
            let directions = usable_directions(self.rules, self.sprite, sequence)?;
            for direction in 0..directions {
                let candidate = Playhead {
                    sequence,
                    direction,
                    position: 0,
                };
                if let Some(found) = self.first_visible_position(candidate) {
                    return Ok(found);
                }
            }
        }
        Err(PlaybackError::new(
            "no sequence of this sprite decodes to any pixels",
        ))
    }

    /// One tick of playback: [`advance`], then keep advancing while the frame shows nothing.
    ///
    /// Returns the new playhead and whether the cycle reported completion on the way. Skipping
    /// blank frames is the viewer's concession to a corpus that has them; it cannot change which
    /// frames the engine would show, only how long the viewer dwells on nothing.
    pub fn tick(&self, head: Playhead) -> Result<(Playhead, bool), PlaybackError> {
        let length = resolve(self.rules, self.sprite, head)?.cycle_length;
        let mut current = head;
        let mut ended = false;
        for _ in 0..length.max(1) {
            let (next, completed) = advance(self.rules, self.sprite, current)?;
            ended |= completed;
            current = next;
            if self.shows(current) {
                return Ok((current, ended));
            }
            // A held cycle cannot move again, so looking further would spin.
            if completed
                && resolve(self.rules, self.sprite, current)?.end == CycleEnd::HoldLastFrame
            {
                break;
            }
        }
        Ok((current, ended))
    }
}

fn sequence_record(
    sprite: &ImpSprite,
    sequence: usize,
) -> Result<&crate::imp::ImpSequence, PlaybackError> {
    sprite.sequences.get(sequence).ok_or_else(|| {
        PlaybackError::new(format!(
            "sequence index {sequence} is past the {} the sprite decodes",
            sprite.sequences.len()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imp_anim::{EngineAddresses, recover};
    use crate::native_table::PeImage;

    use crate::imp::{ImpFacing, ImpFrame, ImpSequence};
    use crate::imp_anim::{CycleModeEntry, CycleModeTable, Parity};

    /// A skeleton sprite: facings of the given frame counts, and nothing else real.
    ///
    /// This is **not** a corpus stand-in and nothing about the shipped archive is asserted
    /// against it -- that is what the `#[ignore]`d tests below are for, and a fixture shaped like
    /// the corpus could not fail on what the corpus hides. Its only job is to let the two tests
    /// that follow ask which *value* the fold consults, which no corpus run can distinguish
    /// because the corpus is played under the real rules by construction.
    fn skeleton(metadata: [u8; 11], facing_frames: &[usize]) -> ImpSprite {
        let mut facings = Vec::new();
        let mut frames = Vec::new();
        for count in facing_frames {
            facings.push(ImpFacing {
                metadata: 0,
                first_frame: frames.len(),
                frame_count: *count,
            });
            for _ in 0..*count {
                frames.push(ImpFrame {
                    flags: 0,
                    width: 1,
                    height: 1,
                    origin_x: None,
                    origin_y: None,
                    hotspots: Vec::new(),
                    palette_indices: vec![0],
                    rgba: vec![0, 0, 0, 0],
                    source_frame: None,
                    record_offset: 0,
                    hotspot_offset: None,
                    packed_size: None,
                    pixels_offset: None,
                    stored_size: None,
                });
            }
        }
        ImpSprite {
            file_flags: 0,
            record_variant: 0,
            compressed: false,
            bits_per_pixel: 8,
            maximum_width: 1,
            maximum_height: 1,
            color_key: 0,
            sequence_count: 1,
            facing_count: facings.len(),
            frame_count: frames.len(),
            duplicate_frame_count: 0,
            back_reference_frame_count: 0,
            hotspot_count: 0,
            hotspot_bytes: 0,
            raw_pixel_bytes: 0,
            stored_pixel_bytes: 0,
            palette: Vec::new(),
            sequences: vec![ImpSequence {
                metadata,
                first_facing: 0,
                facing_count: facings.len(),
                first_frame: 0,
                frame_count: frames.len(),
            }],
            facings,
            frames,
        }
    }

    /// Rules that say what the caller tells them to say, so a test can hand the fold a value the
    /// binary does not hold and see which one it obeyed.
    fn rules_saying(ping_pong_mode: u8, mirror_bit: u8, modes: &[(u8, CycleEnd)]) -> AnimRules {
        AnimRules {
            cycle_modes: CycleModeTable {
                advance: 0,
                dispatch_site: 0,
                table_address: 0,
                modes: modes
                    .iter()
                    .map(|(mode, end)| CycleModeEntry {
                        mode: *mode,
                        target: 0,
                        end: *end,
                    })
                    .collect(),
            },
            ping_pong_mode,
            ping_pong_length_site: 0,
            ping_pong_reflection_site: 0,
            mirror_bit,
            mirror_test_sites: Vec::new(),
            mirror_decrements_when: Parity::Even,
            mirror_parity_site: 0,
        }
    }

    fn walk(rules: &AnimRules, sprite: &ImpSprite, direction: usize, steps: usize) -> Vec<usize> {
        let mut head = Playhead {
            sequence: 0,
            direction,
            position: 0,
        };
        let mut frames = Vec::new();
        for _ in 0..steps {
            frames.push(resolve(rules, sprite, head).expect("resolves").frame);
            head = advance(rules, sprite, head).expect("advances").0;
        }
        frames
    }

    /// The reflection follows the mode the *rules* name, not the mode this crate's constant
    /// names. Point the rules at a mode the sequence does not use and the ping-pong disappears;
    /// point them at the one it does and it comes back. A fold that read
    /// `imp_anim::PING_PONG_MODE` would pass the second half and fail the first.
    #[test]
    fn the_reflection_follows_the_mode_the_rules_name() {
        let sprite = skeleton([4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], &[3]);
        let reflecting = rules_saying(4, 0x80, &[(4, CycleEnd::WrapToStart)]);
        // Frame 0 twice at the seam is the engine's own behaviour, not a slip: the cycle is
        // `2N-1` steps and wraps to position 0, so the traversal visits each endpoint once
        // *within* a cycle and repeats the first frame when the next one starts.
        assert_eq!(walk(&reflecting, &sprite, 0, 6), vec![0, 1, 2, 1, 0, 0]);
        let forward = rules_saying(3, 0x80, &[(4, CycleEnd::WrapToStart)]);
        assert_eq!(walk(&forward, &sprite, 0, 6), vec![0, 1, 2, 0, 1, 2]);
    }

    /// The one-shot holds its last frame instead of wrapping, and which it does comes from the
    /// `CycleEnd` the rules carry rather than from the mode number.
    #[test]
    fn a_held_ending_stops_where_a_wrapping_one_restarts() {
        let sprite = skeleton([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], &[3]);
        let held = rules_saying(4, 0x80, &[(1, CycleEnd::HoldLastFrame)]);
        assert_eq!(walk(&held, &sprite, 0, 5), vec![0, 1, 2, 2, 2]);
        let wrapping = rules_saying(4, 0x80, &[(1, CycleEnd::WrapToStart)]);
        assert_eq!(walk(&wrapping, &sprite, 0, 5), vec![0, 1, 2, 0, 1]);
    }

    /// A mirrored direction resolves to an earlier facing and reports the flip, and the bit that
    /// turns it on is the one the rules carry.
    #[test]
    fn the_mirror_fold_follows_the_bit_the_rules_name() {
        // Three facings, one frame each, mirror bit set: 2 x 3 - 2 = 4 directions, the last of
        // which is facing 1 flipped.
        let sprite = skeleton([0, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 3], &[1, 1, 1]);
        let mirroring = rules_saying(4, 0x80, &[(0, CycleEnd::WrapToStart)]);
        assert_eq!(
            advertised_directions(&mirroring, &sprite, 0).expect("advertised"),
            4
        );
        let mirrored = resolve(
            &mirroring,
            &sprite,
            Playhead {
                sequence: 0,
                direction: 3,
                position: 0,
            },
        )
        .expect("direction 3 resolves");
        assert_eq!((mirrored.facing, mirrored.flipped), (1, true));

        // Point the rules at a bit the record does not set and the sequence stops mirroring.
        let plain = rules_saying(4, 0x40, &[(0, CycleEnd::WrapToStart)]);
        assert_eq!(
            advertised_directions(&plain, &sprite, 0).expect("advertised"),
            3
        );
        assert!(
            resolve(
                &plain,
                &sprite,
                Playhead {
                    sequence: 0,
                    direction: 3,
                    position: 0,
                },
            )
            .is_err(),
            "direction 3 resolved without the mirror bit"
        );
    }

    /// A mode the dispatch does not cover is refused rather than given an invented ending. The
    /// engine does not clamp such a sequence either -- the index runs on and `Imp::GetFrame`'s
    /// own range check fails.
    #[test]
    fn a_mode_outside_the_dispatch_is_refused() {
        let sprite = skeleton([5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], &[3]);
        let rules = rules_saying(4, 0x80, &[(0, CycleEnd::WrapToStart)]);
        let error = resolve(
            &rules,
            &sprite,
            Playhead {
                sequence: 0,
                direction: 0,
                position: 0,
            },
        )
        .expect_err("mode 5 is not dispatched");
        assert!(error.to_string().contains("cycle mode 5"), "{error}");
    }

    fn navigator<'a>(
        rules: &'a AnimRules,
        sprite: &'a ImpSprite,
        visible: &'a dyn Fn(usize) -> bool,
    ) -> Navigator<'a> {
        Navigator {
            rules,
            sprite,
            visible,
        }
    }

    /// Playback skips frames that decode to nothing without leaving the engine's order: the
    /// frames it does show are the ones `resolve` names, in the order `advance` reaches them.
    #[test]
    fn playback_skips_blank_frames_and_keeps_the_engine_order() {
        let sprite = skeleton([4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], &[4]);
        let rules = rules_saying(4, 0x80, &[(4, CycleEnd::WrapToStart)]);
        // Frame 2 decodes to nothing. The seven-step reflecting cycle is 0,1,2,3,2,1,0, so the
        // shown order has to be 0,1,3,1,0 and not 0,1,3,3 or the reflection has been flattened.
        // The sixth entry is frame 0 again: the cycle has just wrapped, and a ping-pong's first
        // and last positions are the same frame.
        let visible = |frame: usize| frame != 2;
        let navigator = navigator(&rules, &sprite, &visible);
        let mut head = navigator.playable_at(0).expect("a playable frame");
        let mut shown = vec![resolve(&rules, &sprite, head).expect("resolves").frame];
        for _ in 0..5 {
            head = navigator.tick(head).expect("ticks").0;
            shown.push(resolve(&rules, &sprite, head).expect("resolves").frame);
        }
        assert_eq!(shown, vec![0, 1, 3, 1, 0, 0]);
    }

    /// A held cycle reports completion and stops moving, which is what lets a caller decide to
    /// stop rather than loop. A `tick` that kept searching for a visible frame would spin here.
    #[test]
    fn a_held_cycle_reports_completion_and_stays_put() {
        let sprite = skeleton([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], &[2]);
        let rules = rules_saying(4, 0x80, &[(1, CycleEnd::HoldLastFrame)]);
        let visible = |_: usize| true;
        let navigator = navigator(&rules, &sprite, &visible);
        let head = Playhead {
            sequence: 0,
            direction: 0,
            position: 1,
        };
        let (next, ended) = navigator.tick(head).expect("ticks");
        assert!(ended, "the one-shot did not report completion");
        assert_eq!(next, head, "the one-shot moved off its last frame");
    }

    /// Turning reaches the mirrored directions, and a scrub stays inside the cycle it is in.
    #[test]
    fn turning_reaches_the_mirrored_directions() {
        let sprite = skeleton([0, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 3], &[1, 1, 1]);
        let rules = rules_saying(4, 0x80, &[(0, CycleEnd::WrapToStart)]);
        let visible = |_: usize| true;
        let navigator = navigator(&rules, &sprite, &visible);
        let mut head = navigator.playable_at(0).expect("a playable frame");
        let mut seen = Vec::new();
        for _ in 0..4 {
            let resolved = resolve(&rules, &sprite, head).expect("resolves");
            seen.push((resolved.frame, resolved.flipped));
            head = navigator.turn(head, 1).expect("turns");
        }
        assert_eq!(
            seen,
            vec![(0, false), (1, false), (2, false), (1, true)],
            "the fourth direction is not facing 1 mirrored"
        );
    }

    /// A direction whose frames all decode to nothing is stepped over rather than landed on.
    #[test]
    fn a_blank_direction_is_stepped_over() {
        let sprite = skeleton([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3], &[1, 1, 1]);
        let rules = rules_saying(4, 0x80, &[(0, CycleEnd::WrapToStart)]);
        let visible = |frame: usize| frame != 1;
        let navigator = navigator(&rules, &sprite, &visible);
        let head = Playhead {
            sequence: 0,
            direction: 0,
            position: 0,
        };
        let turned = navigator.turn(head, 1).expect("turns");
        assert_eq!(turned.direction, 2);
    }

    /// The installed game. `#[ignore]`d rather than silently skipped, for the reason `imp_anim`
    /// gives: a passing test that could not run is indistinguishable from one that did.
    ///
    /// Run with:
    ///   LOM_GAME_DIR=.../English LOM_LISTFILE=... cargo test --release -- --ignored
    fn game_directory() -> std::path::PathBuf {
        let directory = std::env::var_os("LOM_GAME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set LOM_GAME_DIR to the installed English directory");
        assert!(
            directory.join("lomse.exe").is_file(),
            "no lomse.exe under {}",
            directory.display()
        );
        directory
    }

    fn corpus() -> (AnimRules, Vec<(String, ImpSprite)>) {
        let directory = game_directory();
        let exe = std::fs::read(directory.join("lomse.exe")).expect("read lomse.exe");
        let image = PeImage::parse(&exe).expect("parse lomse.exe");
        let rules = recover(&image, &EngineAddresses::default()).expect("rules are recovered");
        let archive = crate::mpq::Archive::open(&directory.join("imp.mpq")).expect("open imp.mpq");
        let listfile =
            std::env::var("LOM_LISTFILE").expect("set LOM_LISTFILE alongside LOM_GAME_DIR");
        let names = std::fs::read_to_string(listfile).expect("read listfile");
        let mut sprites = Vec::new();
        for name in names.lines() {
            if !name.to_ascii_lowercase().ends_with(".imp") {
                continue;
            }
            let Ok(bytes) = archive.read(name) else {
                continue;
            };
            let Ok(sprite) = ImpSprite::parse(&bytes) else {
                continue;
            };
            sprites.push((name.to_owned(), sprite));
        }
        (rules, sprites)
    }

    /// The naive walk the viewer used before this module existed, so the corpus can be asked how
    /// many sequences the two disagree on. It is the *previous implementation*, reproduced here
    /// exactly: step one index inside the current facing and wrap at its end.
    fn naive_walk(facing_frames: usize, steps: usize) -> Vec<usize> {
        (0..steps).map(|step| step % facing_frames.max(1)).collect()
    }

    fn engine_walk(
        rules: &AnimRules,
        sprite: &ImpSprite,
        head: Playhead,
        steps: usize,
    ) -> Result<Vec<usize>, PlaybackError> {
        let facing_first = sprite.facings[resolve(rules, sprite, head)?.facing].first_frame;
        let mut head = head;
        let mut frames = Vec::with_capacity(steps);
        for _ in 0..steps {
            let resolved = resolve(rules, sprite, head)?;
            frames.push(resolved.frame - facing_first);
            head = advance(rules, sprite, head)?.0;
        }
        Ok(frames)
    }

    /// Every sequence in the shipped archive resolves under the rules, at every direction and
    /// every cycle position. Asserted against the corpus rather than against a fixture, because a
    /// fixture shaped like the corpus cannot fail on what the corpus hides.
    #[test]
    #[ignore = "needs LOM_GAME_DIR and LOM_LISTFILE"]
    fn every_shipped_sequence_resolves_at_every_direction_and_position() {
        let (rules, sprites) = corpus();
        let mut sequences = 0_usize;
        let mut resolutions = 0_usize;
        let mut refusals = 0_usize;
        for (name, sprite) in &sprites {
            for index in 0..sprite.sequences.len() {
                sequences += 1;
                let directions =
                    usable_directions(&rules, sprite, index).expect("direction count resolves");
                for direction in 0..directions {
                    let head = Playhead {
                        sequence: index,
                        direction,
                        position: 0,
                    };
                    let first = match resolve(&rules, sprite, head) {
                        Ok(first) => first,
                        Err(error) => {
                            // **Observed in the corpus** 2026-09-19, by a byte-level walk of all
                            // 1,800 `imp.mpq` members that does not share this decoder (`tools/imp_structure_scan.py`):
                            // of the
                            // 14,921 facing records, **none** has `frame_count == 0` -- the same
                            // walk found 6,552 zero-*dimension* frames across 107 files, so it is
                            // not blind to the shape it is looking for. `resolve` therefore refuses
                            // nothing here, which `refusals == 0` below asserts directly.
                            //
                            // **Inferred**, not observed: this arm is a guard for a build whose
                            // archive does hold an empty facing, or whose dispatch covers fewer
                            // modes. It is counted rather than excused, so a refusal of *any* cause
                            // fails the test naming that cause instead of being diverted into a
                            // claim about facings.
                            refusals += 1;
                            eprintln!(
                                "{name} sequence {index} direction {direction} refused: {error}"
                            );
                            continue;
                        }
                    };
                    for position in 0..first.cycle_length {
                        let resolved = resolve(&rules, sprite, Playhead { position, ..head })
                            .unwrap_or_else(|error| {
                                panic!("{name} sequence {index} direction {direction}: {error}")
                            });
                        assert!(resolved.frame < sprite.frames.len());
                        resolutions += 1;
                    }
                }
            }
        }
        assert_eq!(
            refusals, 0,
            "`resolve` refused a sequence/direction this archive advertises; the refusals are \
             printed above"
        );
        assert_eq!(
            sequences, 4_667,
            "the archive is not the one this was measured on"
        );
        assert_eq!(
            resolutions, 97_232,
            "frame resolutions across every sequence, direction and cycle position"
        );
    }

    /// What wiring the fold changed, counted against the shipped archive.
    ///
    /// Each number is a count of sequences the engine rules play differently from the walk the
    /// viewer used before. They are measurements of the corpus, not restatements of the code: set
    /// `PING_PONG_MODE` to anything else and the reflection count goes to zero, and drop the
    /// mirror fold and the synthesised-direction count does.
    #[test]
    #[ignore = "needs LOM_GAME_DIR and LOM_LISTFILE"]
    fn the_fold_changes_these_many_shipped_sequences() {
        let (rules, sprites) = corpus();
        let mut reflecting = 0_usize;
        let mut one_shot = 0_usize;
        let mut mirrored_directions = 0_usize;
        let mut synthesised_facings = 0_usize;
        let mut five_facings_eight_directions = 0_usize;
        let mut differs_from_naive = 0_usize;
        let mut indistinguishable = 0_usize;
        for (_name, sprite) in &sprites {
            for index in 0..sprite.sequences.len() {
                let record = &sprite.sequences[index];
                let head = Playhead {
                    sequence: index,
                    direction: 0,
                    position: 0,
                };
                let Ok(resolved) = resolve(&rules, sprite, head) else {
                    continue;
                };
                if resolved.mode == rules.ping_pong_mode {
                    reflecting += 1;
                }
                if resolved.end == CycleEnd::HoldLastFrame {
                    one_shot += 1;
                }
                let advertised =
                    advertised_directions(&rules, sprite, index).expect("direction count resolves");
                let synthesised = (0..advertised)
                    .filter(|direction| {
                        facing_for_direction(
                            rules.mirror_bit,
                            &record.metadata,
                            record.facing_count,
                            *direction,
                        )
                        .is_some_and(|(_, flipped)| flipped)
                    })
                    .count();
                if synthesised > 0 {
                    mirrored_directions += 1;
                    synthesised_facings += synthesised;
                }
                if record.facing_count == 5 && advertised == 8 {
                    five_facings_eight_directions += 1;
                }
                // The order the two walks produce, for direction 0 alone -- the only direction
                // the old walk could reach at all. Two full cycles and one step more, because a
                // one-shot and a loop agree for exactly one cycle and part company on the step
                // after it.
                let steps = 2 * resolved.cycle_length.max(resolved.facing_frames) + 1;
                let engine = engine_walk(&rules, sprite, head, steps).expect("walk runs");
                if engine != naive_walk(resolved.facing_frames, steps) {
                    differs_from_naive += 1;
                } else if resolved.mode == rules.ping_pong_mode
                    || resolved.end == CycleEnd::HoldLastFrame
                {
                    // A one-frame facing reflects onto itself and holds what it was already
                    // showing, so the rule applies and changes nothing visible. Counted rather
                    // than glossed, because it is why the "changed" total is below 955 + 5.
                    indistinguishable += 1;
                }
            }
        }
        assert_eq!(reflecting, 955, "ping-pong sequences that now reflect");
        assert_eq!(one_shot, 5, "one-shot sequences that no longer loop");
        assert_eq!(
            five_facings_eight_directions, 2_234,
            "five-facing sequences covering eight directions"
        );
        assert_eq!(
            mirrored_directions, 2_304,
            "sequences with at least one synthesised direction"
        );
        // 2,304, not the 2,388 sequences that have the mirror bit set with two or more facings.
        // The 84 two-facing mirrored sequences advertise 2 directions and both are below the
        // facing count, so the fold is never reached and nothing is synthesised. Asserting both
        // keeps that distinction from being lost.
        assert_eq!(
            synthesised_facings, 7_810,
            "synthesised directions across the archive"
        );
        assert_eq!(
            differs_from_naive, 941,
            "sequences whose direction-0 frame order the fold changes"
        );
        // 19, not 0: a one-frame facing reflects onto itself and holds the frame it was already
        // showing. Those 19 obey the rule and look the same doing it.
        assert_eq!(
            indistinguishable, 19,
            "reflecting or one-shot sequences the fold cannot change"
        );
        // Every sequence the fold changes is one of the two the rules single out, and every one
        // of those is either changed or accounted for. A mode-0 sequence that started differing
        // would break this without either count moving on its own.
        assert_eq!(
            differs_from_naive + indistinguishable,
            reflecting + one_shot,
            "a sequence outside the ping-pong and one-shot modes changed, or one inside was lost"
        );
    }

    /// The 991 inert records: the mirror bit set on a single facing advertises zero directions,
    /// and direction 0 still resolves. Pinned because a viewer that trusted the advertised count
    /// would show them nothing.
    #[test]
    #[ignore = "needs LOM_GAME_DIR and LOM_LISTFILE"]
    fn single_facing_mirrored_sequences_advertise_nothing_and_still_play() {
        let (rules, sprites) = corpus();
        let mut inert = 0_usize;
        for (name, sprite) in &sprites {
            for index in 0..sprite.sequences.len() {
                if advertised_directions(&rules, sprite, index).expect("advertised") != 0 {
                    continue;
                }
                if sprite.sequences[index].facing_count == 0 {
                    continue;
                }
                inert += 1;
                assert_eq!(
                    usable_directions(&rules, sprite, index).expect("usable"),
                    1,
                    "{name} sequence {index}"
                );
                let head = Playhead {
                    sequence: index,
                    direction: 0,
                    position: 0,
                };
                let resolved = resolve(&rules, sprite, head)
                    .unwrap_or_else(|error| panic!("{name} sequence {index}: {error}"));
                assert!(
                    !resolved.flipped,
                    "{name} sequence {index} mirrors facing 0"
                );
            }
        }
        assert_eq!(
            inert, 991,
            "single-facing sequences with the mirror bit set"
        );
    }
}
