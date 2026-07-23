#[test]
fn collapses_single_nested_c() {
    assert_eq!(
        super::collapse_nested_lean_ctx_exec("lean-ctx -c 'git status'").as_deref(),
        Some("git status")
    );
}

#[test]
fn collapses_repeated_nested_c() {
    assert_eq!(
        super::collapse_nested_lean_ctx_exec("lean-ctx -c 'lean-ctx -c \"git status\"'").as_deref(),
        Some("git status")
    );
}

#[test]
fn preserves_inner_shell_quoting() {
    assert_eq!(
        super::collapse_nested_lean_ctx_exec("lean-ctx -c \"git commit -m 'hello world'\"")
            .as_deref(),
        Some("git commit -m 'hello world'")
    );
    assert_eq!(
        super::collapse_nested_lean_ctx_exec("lean-ctx -c git commit -m 'hello world'").as_deref(),
        Some("git commit -m 'hello world'")
    );
}

#[test]
fn collapses_exec_alias_and_path() {
    assert_eq!(
        super::collapse_nested_lean_ctx_exec("/usr/local/bin/lean-ctx exec 'git status'")
            .as_deref(),
        Some("git status")
    );
}

#[test]
fn leaves_non_wrappers_alone() {
    assert!(super::collapse_nested_lean_ctx_exec("git status").is_none());
}

#[test]
fn wrapped_nested_wrapper_still_owns_one_compression_pass() {
    let _lock = crate::core::data_dir::test_env_lock();
    crate::test_env::set_var(super::super::super::reentry::WRAP_MARKER, "1");

    assert!(super::should_delegate_wrapped_to_shell_default(false));
    assert!(
        !super::should_delegate_wrapped_to_shell_default(true),
        "collapsed nested wrappers must not fall through to raw shell-default path"
    );

    crate::test_env::remove_var(super::super::super::reentry::WRAP_MARKER);
}
