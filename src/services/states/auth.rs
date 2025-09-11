use uuid::Uuid;

pub struct AuthState {
    basic_auth_key: cookie::Key,
    csrf_auth_key: cookie::Key,
    pre_mfa_auth_key: cookie::Key,
    mfa_auth_key: cookie::Key,
    session_id: Uuid,
}

impl AuthState {
    pub fn new() -> Self {
        let session_id = Uuid::new_v4();
        tracing::info!("AuthState initialized with session_id: {session_id}");
        Self {
            basic_auth_key: cookie::Key::generate(),
            csrf_auth_key: cookie::Key::generate(),
            pre_mfa_auth_key: cookie::Key::generate(),
            mfa_auth_key: cookie::Key::generate(),
            session_id,
        }
    }

    pub fn get_basic_auth_key(&self) -> &cookie::Key {
        &self.basic_auth_key
    }

    pub fn get_csrf_auth_key(&self) -> &cookie::Key {
        &self.csrf_auth_key
    }

    pub fn get_pre_mfa_auth_key(&self) -> &cookie::Key {
        &self.pre_mfa_auth_key
    }

    pub fn get_mfa_auth_key(&self) -> &cookie::Key {
        &self.mfa_auth_key
    }

    pub fn get_session_id(&self) -> &Uuid {
        self.session_id.as_ref()
    }
}
