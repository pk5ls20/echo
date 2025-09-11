pub const ECHO_BASIC_AUTH: &str = "echo_bbbb";
pub const ECHO_CSRF_AUTH: &str = "echo_www";
pub const ECHO_PRE_MFA_AUTH: &str = "echo_prpr";
pub const ECHO_MFA_AUTH: &str = "echo_qwq";
pub const ECHO_BASIC_AUTH_EXPIRE: time::Duration = time::Duration::minutes(30);
pub const ECHO_CSRF_AUTH_EXPIRE: time::Duration = ECHO_BASIC_AUTH_EXPIRE;
pub const ECHO_PRE_MFA_AUTH_EXPIRE: time::Duration = ECHO_BASIC_AUTH_EXPIRE;
pub const ECHO_MFA_AUTH_EXPIRE: time::Duration = time::Duration::minutes(10);
