use crate::iface::{InterfaceContext, UnitIface};
use hd_base::UnitId;

impl<const N: usize> InterfaceContext for [(UnitId, &UnitIface); N] {
    fn unit(&self, id: UnitId) -> Option<&UnitIface> {
        self.as_slice().iter().find(|p| p.0 == id).map(|p| p.1)
    }
    fn iter(&self) -> Box<dyn Iterator<Item = &UnitIface> + '_> {
        Box::new(self.as_slice().iter().map(|p| p.1))
    }
}
