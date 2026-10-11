// Tests for the macros of the zbus-lockstep crate.
//
// - `method_return_signature`
// - `method_args_signature`
// - `signal_body_type_signature`
// - `property_type_signature`

use zbus_lockstep::{
    method_args_signature, method_return_signature, property_type_signature,
    signal_body_type_signature,
};

#[test]
fn test_method_return_signature() {
    let signature = method_return_signature!("RequestName");
    assert_eq!(signature, "u");
}

#[test]
fn test_method_args_signature() {
    let signature = method_args_signature!("RequestName");
    assert_eq!(signature, "su");
}

#[test]
fn test_signal_body_type_signature() {
    let signature = signal_body_type_signature!("RemoveNode");
    assert_eq!(signature, "(so)");
}

#[test]
fn test_property_type_signature() {
    let signature = property_type_signature!("Features");
    assert_eq!(signature, "as");
}
