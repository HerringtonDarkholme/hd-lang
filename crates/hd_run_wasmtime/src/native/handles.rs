//! The instance-local registry of returned native handles
//! (`module.host.package.native-handles`, `cli.host.package.native-handle-release`,
//! `native-handle-check`).
//!
//! Each instance owns one [`Handles`] in its host state, so a number
//! another instance issued resolves only against this instance's own
//! entries: the instance is the owner, as its reactor's inbox is the
//! owner of its pending calls (#2081). An entry records the contract
//! interface of its generated handle trait, that trait's dispatch, and the
//! trait object. The host's own handles that a std handle trait names
//! (`hd_host_abi::HANDLES` rows, as `std.fs.File`) live here too, under
//! an interface number no contract issues and with no dispatch: their
//! methods are host methods, which reach the value with [`Handles::host`].
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
pub(crate) const SLOTS: u32 = (1 << Handle::SLOT_BITS) - 1;
/// Generations per slot; a slot whose generations are spent retires.
pub(crate) const GENERATIONS: u32 = 1 << Handle::GEN_BITS;

/// A live entry: its generated handle trait, that trait's dispatch (none
/// for a host handle), and its trait object, which is out of its slot
/// while one of its methods runs.
struct Entry {
    interface: u32,
    dispatch: Option<Dispatch>,
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

pub(crate) fn fail(message: &str) -> HostError {
    HostError(format!("package host: {message}"))
}

impl Handles {
    /// The live entries, counted for a test.
    #[cfg(feature = "testing")]
    pub(crate) fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.entry.is_some()).count()
    }

    /// The slots issued so far, live or not, for a test.
    #[cfg(feature = "testing")]
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Fills the table to `count` slots with vacant ones at `generation`,
    /// none of them free, for a test.
    #[cfg(feature = "testing")]
    pub(crate) fn fill_vacant(&mut self, count: usize, generation: u32) {
        self.slots.resize_with(count, || Slot {
            generation,
            entry: None,
        });
    }

    /// Marks `slot` free for reuse, for a test.
    #[cfg(feature = "testing")]
    pub(crate) fn free_slot(&mut self, slot: u32) {
        self.free.push(slot);
    }

    /// Issues a number for `value`, a trait object of the handle trait at
    /// contract interface `interface`, whose methods `dispatch` calls.
    pub(crate) fn insert(
        &mut self,
        interface: u32,
        dispatch: Dispatch,
        value: Box<dyn Any>,
    ) -> Result<u32, HostError> {
        self.issue(interface, Some(dispatch), value)
    }

    /// Issues a number for a host handle's `value`, under `interface`,
    /// one of the host's numbers above every contract's interfaces.
    pub(crate) fn insert_host(
        &mut self,
        interface: u32,
        value: Box<dyn Any>,
    ) -> Result<u32, HostError> {
        self.issue(interface, None, value)
    }

    /// The value of the live host handle `number` of `interface`, if it is
    /// one and holds a `T`.
    pub(crate) fn host<T: 'static>(&mut self, number: u32, interface: u32) -> Option<&mut T> {
        self.entry(number, interface)?
            .entry
            .as_mut()?
            .value
            .as_mut()?
            .downcast_mut::<T>()
    }

    fn issue(
        &mut self,
        interface: u32,
        dispatch: Option<Dispatch>,
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
        // A host handle has no dispatch: its interface is the host's, so
        // no contract method names it; refused rather than assumed.
        let dispatch = entry.dispatch.ok_or_else(invalid)?;
        // ruled out: no guest code runs while a method is lent, so no call
        // reaches a lent handle; refused rather than assumed.
        let value = entry.value.take().ok_or_else(invalid)?;
        Ok(Lent {
            number,
            dispatch,
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
