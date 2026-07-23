#[cfg(test)]
pub(crate) struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: EnvVarGuard is test-only; env-mutating tests are serialized,
        // so no other thread reads or writes the environment concurrently.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

#[cfg(test)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            // SAFETY: EnvVarGuard is test-only; env-mutating tests are serialized,
            // so no other thread reads or writes the environment concurrently.
            unsafe { std::env::set_var(self.key, previous) };
        } else {
            // SAFETY: EnvVarGuard is test-only; env-mutating tests are serialized,
            // so no other thread reads or writes the environment concurrently.
            unsafe { std::env::remove_var(self.key) };
        }
    }
}
