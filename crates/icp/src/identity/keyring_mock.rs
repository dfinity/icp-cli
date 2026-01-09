use std::fmt::Debug;

use keyring::{
    Credential,
    credential::{CredentialApi, CredentialBuilderApi, CredentialPersistence},
};

use crate::prelude::*;

pub struct MockKeyring {
    pub dir: PathBuf,
}

impl CredentialBuilderApi for MockKeyring {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn build(
        &self,
        target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        let filename = format!(
            "{}_{}_{}.mockcred",
            target.unwrap_or("default"),
            service,
            user.replace(|c: char| !c.is_alphanumeric(), "_"),
        );
        let filepath = self.dir.join(filename);
        Ok(Box::new(MockCredential { file: filepath }))
    }
    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::UntilDelete
    }
}

#[derive(Debug)]
pub struct MockCredential {
    pub file: PathBuf,
}

impl CredentialApi for MockCredential {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn get_password(&self) -> keyring::Result<String> {
        std::fs::read_to_string(&self.file)
            .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))
    }
    fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        std::fs::remove_file(&self.file).map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))
    }
    fn get_attributes(&self) -> keyring::Result<std::collections::HashMap<String, String>> {
        Err(keyring::Error::PlatformFailure("unsupported".into()))
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        std::fs::read(&self.file).map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))
    }
    fn set_password(&self, password: &str) -> keyring::Result<()> {
        std::fs::write(&self.file, password.as_bytes())
            .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))
    }
    fn set_secret(&self, password: &[u8]) -> keyring::Result<()> {
        std::fs::write(&self.file, password)
            .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))
    }
    fn update_attributes(&self, _: &std::collections::HashMap<&str, &str>) -> keyring::Result<()> {
        Err(keyring::Error::PlatformFailure("unsupported".into()))
    }
}
