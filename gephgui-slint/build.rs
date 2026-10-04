use std::io;
#[cfg(windows)]
use winres::WindowsResource;

fn main() -> io::Result<()> {
    // complie slint
    slint_build::compile("ui/geph_window.slint").unwrap();

    // The version shown in-app and used by the auto-updater is read via
    // option_env!("VERSION") at compile time. Track it so bumping VERSION forces
    // a rebuild instead of baking in a stale (or missing) value from the cache.
    println!("cargo:rerun-if-env-changed=VERSION");

    #[cfg(windows)]
    {
        WindowsResource::new()
            // This path can be absolute, or relative to your crate root.
            .set_icon("ui/resources/logos/logo-win.ico")
            // rfd's custom-text dialogs use TaskDialogIndirect, which requires
            // the version 6 Windows common controls activation context.
            .set_manifest(
                r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <assemblyIdentity
        type="win32"
        name="Geph.Geph"
        version="1.0.0.0"
        processorArchitecture="*"
    />
    <dependency>
        <dependentAssembly>
            <assemblyIdentity
                type="win32"
                name="Microsoft.Windows.Common-Controls"
                version="6.0.0.0"
                processorArchitecture="*"
                publicKeyToken="6595b64144ccf1df"
                language="*"
            />
        </dependentAssembly>
    </dependency>
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false" />
            </requestedPrivileges>
        </security>
    </trustInfo>
</assembly>
"#,
            )
            .compile()?;
    }

    Ok(())

}
