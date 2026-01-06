use forgeerp_auth::{CommandAuthorization, Permission};

/// Small helper wrapper to associate required permissions with a command.
pub struct CmdAuth<C> {
    pub inner: C,
    pub required: Vec<Permission>,
}

impl<C> CommandAuthorization for CmdAuth<C> {
    fn required_permissions(&self) -> &[Permission] {
        &self.required
    }
}


