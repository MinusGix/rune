use crate::hash::Hash;
use crate::value::slot;
use crate::value::slot::{IntoSlot, Slot};
use crate::value::{Managed, ValueType, ValueTypeInfo};
use crate::vm::{StackError, Vm};

/// An entry on the stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValuePtr {
    /// An empty unit.
    Unit,
    /// A boolean.
    Bool(bool),
    /// A character.
    Char(char),
    /// A number.
    Integer(i64),
    /// A float.
    Float(f64),
    /// A managed reference.
    Managed(Slot),
    /// A type.
    Type(Hash),
    /// A pointer to an absolute stack location.
    ///
    /// A pointer is only allowed to point to a lower stack location.
    Ptr(usize),
    /// A function pointer.
    Fn(Hash),
}

impl ValuePtr {
    /// Convert value into a managed slot.
    #[inline]
    fn into_slot<T>(self, vm: &Vm) -> Result<Slot, StackError>
    where
        T: IntoSlot,
    {
        T::into_slot(self, vm)
    }

    /// Try to coerce value reference into an external.
    pub fn into_external(self, vm: &Vm) -> Result<Slot, StackError> {
        self.into_slot::<slot::ExternalSlot>(vm)
    }

    /// Try to coerce value reference into an array.
    pub fn into_array(self, vm: &Vm) -> Result<Slot, StackError> {
        self.into_slot::<slot::ArraySlot>(vm)
    }

    /// Try to coerce value reference into an object.
    pub fn into_object(self, vm: &Vm) -> Result<Slot, StackError> {
        self.into_slot::<slot::ObjectSlot>(vm)
    }

    /// Try to coerce value reference into an array.
    pub fn into_string(self, vm: &Vm) -> Result<Slot, StackError> {
        self.into_slot::<slot::StringSlot>(vm)
    }

    /// Get the type information for the current value.
    pub fn value_type(&self, vm: &Vm) -> Result<ValueType, StackError> {
        Ok(match *self {
            Self::Unit => ValueType::Unit,
            Self::Integer(..) => ValueType::Integer,
            Self::Float(..) => ValueType::Float,
            Self::Bool(..) => ValueType::Bool,
            Self::Char(..) => ValueType::Char,
            Self::Managed(slot) => match slot.into_managed() {
                Managed::String => ValueType::String,
                Managed::Array => ValueType::Array,
                Managed::Object => ValueType::Object,
                Managed::External => ValueType::External(vm.slot_type_id(slot)?),
            },
            Self::Type(..) => ValueType::Type,
            Self::Ptr(..) => ValueType::Ptr,
            Self::Fn(hash) => ValueType::Fn(hash),
        })
    }

    /// Get the type information for the current value.
    pub fn type_info(&self, vm: &Vm) -> Result<ValueTypeInfo, StackError> {
        Ok(match *self {
            Self::Unit => ValueTypeInfo::Unit,
            Self::Integer(..) => ValueTypeInfo::Integer,
            Self::Float(..) => ValueTypeInfo::Float,
            Self::Bool(..) => ValueTypeInfo::Bool,
            Self::Char(..) => ValueTypeInfo::Char,
            Self::Managed(slot) => match slot.into_managed() {
                Managed::String => ValueTypeInfo::String,
                Managed::Array => ValueTypeInfo::Array,
                Managed::Object => ValueTypeInfo::Object,
                Managed::External => ValueTypeInfo::External(vm.slot_type_name(slot)?),
            },
            Self::Type(..) => ValueTypeInfo::Type,
            Self::Ptr(..) => ValueTypeInfo::Ptr,
            Self::Fn(hash) => ValueTypeInfo::Fn(hash),
        })
    }
}

impl Default for ValuePtr {
    fn default() -> Self {
        Self::Unit
    }
}

#[cfg(test)]
mod tests {
    use super::ValuePtr;

    #[test]
    fn test_size() {
        assert_eq! {
            std::mem::size_of::<ValuePtr>(),
            16,
        };
    }
}
