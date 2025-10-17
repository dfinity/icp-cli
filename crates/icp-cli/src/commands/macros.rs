/// Implements `From<&'a Struct>` for a tuple of references to its fields.
#[macro_export]
macro_rules! impl_from_args {
    // Rule for a single field conversion
    ($struct_name:ident, $field:ident: $type:ty) => {
        impl<'a> From<&'a $struct_name> for (&'a $type,) {
            fn from(args: &'a $struct_name) -> Self {
                (&args.$field,)
            }
        }
    };

    // Rule for multiple field conversions
    ($struct_name:ident, $($field:ident: $type:ty),+) => {
        impl<'a> From<&'a $struct_name> for ($(&'a $type),+) {
            fn from(args: &'a $struct_name) -> Self {
                ($(&args.$field),+)
            }
        }
    };
}
