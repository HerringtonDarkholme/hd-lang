use crate::lookup::{UnitContext, UnitImpls};

impl<const N: usize> UnitContext for [(u32, &UnitImpls); N] {
    fn get(&self, id: usize) -> Option<&UnitImpls> {
        self.as_slice()
            .binary_search_by_key(&id, |p| p.0 as usize)
            .ok()
            .map(|i| self[i].1)
    }
    fn units_at(
        &self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &UnitImpls)> + '_> {
        Box::new(self.as_slice().iter().filter_map(move |&(id, table)| {
            crate::work::view_slots(1);
            table
                .table
                .has_rows_at(codes)
                .then_some((id as usize, table))
        }))
    }
}
impl UnitContext for Vec<(u32, &UnitImpls)> {
    fn get(&self, id: usize) -> Option<&UnitImpls> {
        self.as_slice()
            .binary_search_by_key(&id, |p| p.0 as usize)
            .ok()
            .map(|i| self[i].1)
    }
    fn units_at(
        &self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &UnitImpls)> + '_> {
        Box::new(self.as_slice().iter().filter_map(move |&(id, table)| {
            crate::work::view_slots(1);
            table
                .table
                .has_rows_at(codes)
                .then_some((id as usize, table))
        }))
    }
}
