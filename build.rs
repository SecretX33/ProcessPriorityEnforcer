use chrono::Datelike;

fn main() {
    #[cfg(windows)]
    {
        let year = chrono::Utc::now().year();
        let version = env!("CARGO_PKG_VERSION");

        let mut res = winresource::WindowsResource::new();
        #[cfg(not(debug_assertions))]
        {
            res.set_manifest(
                r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
        </requestedPrivileges>
    </security>
</trustInfo>
</assembly>
"#,
            );
        }
        res.set("FileVersion", &format!("{version}.0")); // 0.1.0.0
        res.set("ProductVersion", version); // 0.1.0
        res.set("ProductName", "ProcessPriorityEnforcer");
        res.set("FileDescription", "ProcessPriorityEnforcer");
        res.set("OriginalFilename", "processpriorityenforcer.exe");
        res.set("CompanyName", "SecretX33");
        res.set("LegalCopyright", &format!("Copyright © {year} SecretX33"));
        res.set_icon("icons/icon.ico");
        res.compile().unwrap();
    }
}
