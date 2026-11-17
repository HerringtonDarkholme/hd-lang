//! The instance-local registry of returned native handles
//! (`module.host.package.native-handles`, `cli.host.package.native-handle-release`,
//! `native-handle-check`).
//!
//! Each instance owns one [`Handles`] in its host state, so a number
//! another instance issued resolves only against this instance's own
//! entries: the instance is the owner, as its reactor's inbox is the
//! owner of its pending calls (#2081). An entry records the contract
//! interface of its generated handle trait, that trait's dispatch, and the
//! trait object.
//!
//! The table follows slotmap's `SlotMap`: a vector of slots, each with a
//! generation, and a free list of the vacant ones. A handle number is the
//! operation handles' layout (`hd_host_abi::Handle`: 20 bits of slot, 11
//! of generation) plus one, so it is a positive `i32` and the first is 1,
//! as the JS host's are. Releasing a slot bumps its generation, so the released
//! number never resolves again, even after the slot holds a later entry.
//! Unlike slotmap, whose version wraps, a slot whose generation is spent
//! is retired rather than reused: a number is never issued twice. That
//! bounds an instance to `2^31 - 1` issued handles, the JS host's bound
//! (`handle space exhausted`), and to `2^20 - 1` live ones at once.
//!
//! Cost: insert, lookup, lend, return and release are O(1); nothing scans
//! the table.

use std::any::Any;

use hd_host_abi::Handle;

use super::{Dispatch, HostError};

/// Slots: the last slot index stays unused, so the largest number, plus
/// one, is still a positive `i32`.
const SLOTS: u32 = (1 << Handle::SLOT_BITS) - 1;
/// Generations per slot; a slot whose generations are spent retires.
const GENERATIONS: u32 = 1 << Handle::GEN_BITS;

/// A live entry: its generated handle trait, that trait's dispatch, and
/// its trait object, which is out of its slot while one of its methods
/// runs.
struct Entry {
    interface: u32,
    dispatch: Dispatch,
    value: Option<Box<dyn Any>>,
}

struct Slot {
    generation: u32,
    entry: Option<Entry>,
}

/// One instance's returned native handles. A companion's dispatch passes
/// it to the encoder of a result; only the engine reads it.
#[derive(Default)]
pub struct Handles {
    slots: Vec<Slot>,
    /// Vacant slots that may be reused, most recently freed last.
    free: Vec<u32>,
}

/// A handle's trait object, out of its slot for one method call.
pub(crate) struct Lent {
    number: u32,
    pub(crate) dispatch: Dispatch,
    pub(crate) value: Box<dyn Any>,
}

fn fail(message: &str) -> HostError {
    HostError(format!("package host: {message}"))
}

impl Handles {
    /// The live entries, counted for a test.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.entry.is_some()).count()
    }

    /// Issues a number for `value`, a trait object of the handle trait at
    /// contract interface `interface`, whose methods `dispatch` calls.
    pub(crate) fn insert(
        &mut self,
        interface: u32,
        dispatch: Dispatch,
        value: Box<dyn Any>,
    ) -> Result<u32, HostError> {
        let entry = Entry {
            interface,
            dispatch,
            value: Some(value),
        };
        let index = if let Some(index) = self.free.pop() {
            index
        } else {
            let index = u32::try_from(self.slots.len())
                .ok()
                .filter(|n| *n < SLOTS)
                .ok_or_else(|| fail("handle space exhausted"))?;
            self.slots.push(Slot {
                generation: 0,
                entry: None,
            });
            index
        };
        let slot = &mut self.slots[index as usize];
        slot.entry = Some(entry);
        Ok(Handle::new(index, slot.generation).0 + 1)
    }

    /// The slot `number` names, if `number` is valid.
    fn slot(number: u32) -> Option<Handle> {
        Some(Handle(number.checked_sub(1)?))
    }

    /// The live entry `number` names, if it is of `interface`.
    fn entry(&mut self, number: u32, interface: u32) -> Option<&mut Slot> {
        let handle = Self::slot(number)?;
        let slot = self.slots.get_mut(handle.slot() as usize)?;
        (slot.generation == handle.generation()
            && slot
                .entry
                .as_ref()
                .is_some_and(|e| e.interface == interface))
        .then_some(slot)
    }

    /// Takes the trait object of handle `number` out of its slot for a
    /// method call of `interface`. A number this instance never issued,
    /// one released, or one of another handle trait calls nothing
    /// (`native-handle-check`).
    pub(crate) fn lend(&mut self, number: u32, interface: u32) -> Result<Lent, HostError> {
        let invalid = || fail("call of an invalid handle");
        let entry = self
            .entry(number, interface)
            .and_then(|slot| slot.entry.as_mut())
            .ok_or_else(invalid)?;
        // ruled out: no guest code runs while a method is lent, so no call
        // reaches a lent handle; refused rather than assumed.
        let value = entry.value.take().ok_or_else(invalid)?;
        Ok(Lent {
            number,
            dispatch: entry.dispatch,
            value,
        })
    }

    /// Puts a lent trait object back in its slot after its call.
    pub(crate) fn restore(&mut self, lent: Lent) {
        let index = Handle(lent.number - 1).slot();
        if let Some(Entry { value, .. }) = self.slots[index as usize].entry.as_mut() {
            *value = Some(lent.value);
        }
    }

    /// Releases handle `number` of `interface`: its trait object drops
    /// here, with no provider call (`native-handle-release`). The slot's
    /// next generation makes the number dead for good.
    pub(crate) fn release(&mut self, number: u32, interface: u32) -> Result<(), HostError> {
        let invalid = || fail("release of an invalid handle");
        let slot = self.entry(number, interface).ok_or_else(invalid)?;
        let entry = slot.entry.take();
        slot.generation += 1;
        let index = Handle(number - 1).slot();
        if slot.generation < GENERATIONS {
            self.free.push(index);
        }
        drop(entry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{Answer, HostResult};

    #[allow(clippy::unnecessary_wraps)]
    fn none(_: &mut dyn Any, _: usize, _: &[u8], _: &mut Handles) -> HostResult<Answer> {
        Ok(Answer::Ready(Vec::new()))
    }

    #[test]
    fn a_released_number_never_resolves_again_after_its_slot_is_reused() {
        let mut h = Handles::default();
        let a = h.insert(0, none, Box::new(1u8)).unwrap();
        h.release(a, 0).unwrap();
        let b = h.insert(0, none, Box::new(2u8)).unwrap();
        assert_ne!(a, b, "the reused slot issues a new number");
        assert_eq!(
            Handle(a - 1).slot(),
            Handle(b - 1).slot(),
            "the slot is reused"
        );
        assert!(h.lend(a, 0).is_err());
        assert!(h.release(a, 0).is_err());
        let lent = h.lend(b, 0).unwrap();
        assert_eq!(lent.value.downcast_ref::<u8>(), Some(&2));
        h.restore(lent);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn a_forged_zero_or_wrong_trait_number_resolves_nothing() {
        let mut h = Handles::default();
        let a = h.insert(3, none, Box::new(())).unwrap();
        for forged in [0, a + 1, u32::MAX, a + (1 << Handle::SLOT_BITS)] {
            assert_eq!(
                h.lend(forged, 3).err(),
                Some(fail("call of an invalid handle"))
            );
        }
        assert_eq!(
            h.release(a, 4).err(),
            Some(fail("release of an invalid handle"))
        );
        // A lent handle is not callable again until restored.
        let lent = h.lend(a, 3).unwrap();
        assert!(h.lend(a, 3).is_err());
        h.restore(lent);
        h.release(a, 3).unwrap();
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn a_slot_whose_generations_are_spent_is_retired() {
        let mut h = Handles::default();
        let mut seen = hd_base::HashSet::default();
        for _ in 0..GENERATIONS {
            let n = h.insert(0, none, Box::new(())).unwrap();
            assert!(n > 0 && i32::try_from(n).is_ok());
            assert!(seen.insert(n), "number {n} issued twice");
            h.release(n, 0).unwrap();
        }
        assert_eq!(h.slots.len(), 1);
        let n = h.insert(0, none, Box::new(())).unwrap();
        assert_eq!(h.slots.len(), 2, "the spent slot is not reused");
        assert!(seen.insert(n));
    }

    #[test]
    fn the_table_is_bounded_by_its_slots() {
        let mut h = Handles::default();
        h.slots.resize_with(SLOTS as usize, || Slot {
            generation: GENERATIONS - 1,
            entry: None,
        });
        assert_eq!(
            h.insert(0, none, Box::new(())).err(),
            Some(fail("handle space exhausted"))
        );
        // The largest number fits a positive i32.
        h.free.push(SLOTS - 1);
        let n = h.insert(0, none, Box::new(())).unwrap();
        assert_eq!(n, i32::MAX as u32);
    }
}
