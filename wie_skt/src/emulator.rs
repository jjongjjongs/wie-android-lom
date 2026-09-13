use alloc::{
    borrow::ToOwned,
    boxed::Box,
    collections::BTreeMap,
    format, str,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use jvm::{Result as JvmResult, runtime::JavaLangString};

use wie_backend::{DefaultTaskRunner, Emulator, Event, Platform, System, TitlePlatform, extract_zip, title_quirks};
use wie_jvm_support::{JvmSupport, RustJavaJvmImplementation};
use wie_util::{Result, WieError};

pub struct SktEmulator {
    system: System,
}

impl SktEmulator {
    pub fn from_archive(platform: Box<dyn Platform>, files: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let msd_file = files.iter().find(|x| x.0.ends_with(".msd")).unwrap();
        let msd = SktMsd::parse(msd_file.0, msd_file.1);

        tracing::info!("Loading app {}, mclass {}", msd.id, msd.main_class);

        let jar_filename = msd_file.0.replace(".msd", ".jar");

        // An empty main class is a descriptor that did not carry one. Passing
        // it through as `Some("")` would reach `resolve_class("")`; `None` is
        // what `do_start` reports as a missing main class.
        let main_class = if msd.main_class.is_empty() { None } else { Some(msd.main_class) };

        Self::load(platform, &jar_filename, &msd.id, main_class, msd.properties, &files)
    }

    pub fn from_jar(platform: Box<dyn Platform>, jar_filename: &str, jar: Vec<u8>, id: &str, main_class_name: Option<String>) -> Result<Self> {
        let files = [(jar_filename.to_owned(), jar)].into_iter().collect();

        Self::load(platform, jar_filename, id, main_class_name, BTreeMap::new(), &files)
    }

    pub fn loadable_archive(files: &BTreeMap<String, Vec<u8>>) -> bool {
        files.iter().any(|x| x.0.ends_with(".msd"))
    }

    pub fn loadable_jar(jar: &[u8]) -> bool {
        jar.starts_with(b"\x20\x00\x00\x00\x00\x00\x00\x00")
    }

    /// The panel this archive's title was drawn for, when that is not the one
    /// a host would pick by default.
    ///
    /// A host has to size its screen before there is an emulator to ask, so
    /// this reads the archive's own `.msd` and answers from the quirk table.
    /// `None` means the title has nothing to say and the host's own default
    /// stands, which is every SK-VM title until one is shown to need
    /// otherwise.
    pub fn screen_size(archive: &[u8]) -> Option<(u32, u32)> {
        let files = extract_zip(archive).ok()?;
        let (filename, data) = files.iter().find(|x| x.0.ends_with(".msd"))?;

        title_quirks(TitlePlatform::Skt, &SktMsd::parse(filename, data).id).screen_size
    }

    fn load(
        platform: Box<dyn Platform>,
        jar_filename: &str,
        id: &str,
        main_class_name: Option<String>,
        properties: BTreeMap<String, String>,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Self> {
        let system = System::new(platform, id, id, DefaultTaskRunner);

        // SK-VM titles ask for archive entries in a case the archive does not
        // use - they ship `Data/Map01.dat` and open `data/map01.dat`. The
        // reference emulator resolves those case-insensitively
        // (`aram-core/loader/skvm.findCaseInsensitive`); without it the read
        // simply misses and the title stops at a resource it can see.
        system.filesystem().enable_case_insensitive_reads();

        for (filename, data) in files {
            system.filesystem().add_virtual(filename, data.clone())
        }

        let mut system_clone = system.clone();
        let jar_filename_clone = jar_filename.to_owned();

        system.spawn(async move || Self::do_start(&mut system_clone, jar_filename_clone, properties, main_class_name).await);

        Ok(Self { system })
    }

    #[tracing::instrument(name = "start", skip_all)]
    async fn do_start(
        system: &mut System,
        jar_filename: String,
        properties: BTreeMap<String, String>,
        main_class_name: Option<String>,
    ) -> Result<()> {
        let system_properties = [
            ("MIN", "01000000000"),
            ("m.MIN", "01000000000"),
            ("m.COLOR", "7"),
            ("m.VENDER", "vender"),
            ("m.CARRIER", "SKT"),
            ("m.SK_VM", "10"),
            ("com.xce.wipi.version", ""),
        ];
        let properties = properties
            .into_iter()
            .map(|(k, v)| (format!("wie.appProperty.{k}"), v))
            .collect::<Vec<_>>();
        let properties = system_properties
            .into_iter()
            .chain(properties.iter().map(|(k, v)| (k.as_ref(), v.as_ref())))
            .collect::<Vec<_>>();

        let protos = [
            wie_midp::get_protos().into(),
            wie_skvm::get_protos().into(),
            wie_wipi_java::get_protos().into(),
        ];
        let jvm = JvmSupport::new_jvm(system, Some(&jar_filename), Box::new(protos), &properties, RustJavaJvmImplementation).await?;

        let main_class_name = if let Some(x) = main_class_name {
            x.replace('.', "/")
        } else {
            return Err(WieError::FatalError("Main class not found".into()))?;
        };

        let main_class = jvm.resolve_class(&main_class_name).await.unwrap();
        let main_class_java = JavaLangString::from_rust_string(&jvm, &main_class_name).await.unwrap();

        let result: JvmResult<()> = if jvm.is_inherited_from(&*main_class.definition, "javax/microedition/midlet/MIDlet") {
            jvm.invoke_static("net/wie/Launcher", "start", "(Ljava/lang/String;)V", (main_class_java,))
                .await
        } else {
            let mut args = jvm.instantiate_array("Ljava/lang/String;", 1).await.unwrap();
            jvm.store_array(&mut args, 0, vec![main_class_java]).await.unwrap();
            jvm.invoke_static("org/kwis/msp/lcdui/Main", "main", "([Ljava/lang/String;)V", (args,))
                .await
        };

        if let Err(x) = result {
            return Err(JvmSupport::to_wie_err(&jvm, x).await);
        }

        Ok(())
    }
}

impl Emulator for SktEmulator {
    fn handle_event(&mut self, event: Event) {
        self.system.event_queue().push(event)
    }

    fn tick(&mut self) -> Result<()> {
        self.system.tick()
    }
}

struct SktMsd {
    id: String,
    main_class: String,
    properties: BTreeMap<String, String>,
}

impl SktMsd {
    /// Parses a `.msd` descriptor.
    ///
    /// The format is one `key: value` per line, but the values are not written
    /// to a fixed shape - `MIDlet-1` may or may not have a space after the
    /// colon, lines may end with CRLF, and a truncated descriptor may not
    /// carry the main class at all. The reference loader answers that with a
    /// descriptor parser that reports a format error
    /// (`aram-core/loader/skvm.ParseDescriptor`); here every field is optional
    /// and a missing main class is caught later, where it can be reported,
    /// rather than by indexing off the end of a line.
    pub fn parse(filename: &str, data: &[u8]) -> Self {
        let mut main_class = String::new();
        let mut id: String = filename.split('.').next().unwrap_or(filename).into();
        let mut properties = BTreeMap::new();

        for line in data.split(|x| *x == b'\n') {
            let Ok(line) = str::from_utf8(line) else {
                continue;
            };

            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());

            match key {
                // `MIDlet-1: <name>, <icon>, <class>`. The icon is routinely
                // empty and the name routinely holds no comma, but neither is
                // guaranteed, so the class is taken as the third field and
                // nothing is assumed about the ones before it.
                "MIDlet-1" => {
                    if let Some(class) = value.split(',').nth(2) {
                        main_class = class.trim().to_string();
                    }
                }
                "DD-ProgName" => id = value.to_string(),
                _ => {}
            }

            tracing::info!("Adding property {}={}", key, value);
            properties.insert(key.to_string(), value.to_string());
        }

        Self { id, main_class, properties }
    }
}

#[cfg(test)]
mod tests {
    use super::SktMsd;

    #[test]
    fn a_descriptor_gives_up_its_id_and_main_class() {
        let msd = SktMsd::parse("app.msd", b"MIDlet-1: Game, , com.example.Main\nDD-ProgName: SK0123\n");

        assert_eq!(msd.id, "SK0123");
        assert_eq!(msd.main_class, "com.example.Main");
        assert_eq!(msd.properties.get("MIDlet-1").map(|x| x.as_str()), Some("Game, , com.example.Main"));
    }

    /// Descriptors come with CRLF line endings and without the space after the
    /// colon just as often as with it.
    #[test]
    fn crlf_and_a_missing_space_after_the_colon_read_the_same() {
        let msd = SktMsd::parse("app.msd", b"MIDlet-1:Game,,com.example.Main\r\nDD-ProgName:SK0123\r\n");

        assert_eq!(msd.id, "SK0123");
        assert_eq!(msd.main_class, "com.example.Main");
    }

    /// Without `DD-ProgName` the descriptor filename is the id, and a filename
    /// with no extension is still a usable one.
    #[test]
    fn the_filename_stands_in_for_a_missing_prog_name() {
        assert_eq!(SktMsd::parse("SK9999.msd", b"MIDlet-1: G, , C\n").id, "SK9999");
        assert_eq!(SktMsd::parse("SK9999", b"").id, "SK9999");
    }

    /// A truncated `MIDlet-1` used to be read by slicing past its end. It now
    /// leaves the main class empty, which `do_start` reports.
    #[test]
    fn a_truncated_midlet_line_leaves_the_main_class_empty() {
        let msd = SktMsd::parse("app.msd", b"MIDlet-1: Game\nMIDlet-2\n");

        assert!(msd.main_class.is_empty());
    }

    /// A line with no colon is not a property, and a descriptor made of them
    /// parses to nothing rather than panicking.
    #[test]
    fn lines_without_a_separator_are_skipped() {
        let msd = SktMsd::parse("app.msd", b"garbage\n\nmore garbage\n");

        assert!(msd.properties.is_empty());
        assert!(msd.main_class.is_empty());
    }
}
