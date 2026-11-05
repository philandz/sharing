use sharing::manager::client::ensure_writable;
use tonic::Code;

#[test]
fn active_budget_is_writable() {
    assert!(ensure_writable("active").is_ok());
    assert!(ensure_writable("").is_ok());
}

#[test]
fn archived_budget_rejects_writes() {
    let e = ensure_writable("archived").unwrap_err();
    assert_eq!(e.code(), Code::FailedPrecondition);
}

#[test]
fn closed_or_deleted_budget_rejects_writes() {
    assert_eq!(
        ensure_writable("closed").unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        ensure_writable("deleted").unwrap_err().code(),
        Code::FailedPrecondition
    );
}
