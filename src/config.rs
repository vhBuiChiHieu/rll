use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Config {
    pub(crate) show_hidden: bool,
    pub(crate) sort_field: SortField,
    pub(crate) sort_direction: SortDirection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SortField {
    Unsorted,
    Name,
    Size,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SortDirection {
    Asc,
    Desc,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show_hidden: false,
            sort_field: SortField::Unsorted,
            sort_direction: SortDirection::Desc,
        }
    }
}

impl Config {
    pub(crate) fn load() -> (Self, Option<String>) {
        match config_path() {
            Some(path) => Self::load_from_path(&path),
            None => (
                Self::default(),
                Some("warning: cannot locate user config directory".to_owned()),
            ),
        }
    }

    pub(crate) fn load_from_path(path: &Path) -> (Self, Option<String>) {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) if err.kind() == io::ErrorKind::NotFound => (Self::default(), None),
            Err(err) => (
                Self::default(),
                Some(format!(
                    "warning: cannot read config {}: {err}",
                    path.display()
                )),
            ),
        }
    }

    pub(crate) fn save(&self) -> io::Result<PathBuf> {
        let path = config_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cannot locate user config directory",
            )
        })?;
        self.save_to_path(&path)?;
        Ok(path)
    }

    pub(crate) fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.serialize())
    }

    fn parse(text: &str) -> (Self, Option<String>) {
        let mut config = Self::default();
        let mut warning = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                warning = Some("warning: config contains invalid lines".to_owned());
                continue;
            };

            match (key.trim(), value.trim()) {
                ("show_hidden", "true") => config.show_hidden = true,
                ("show_hidden", "false") => config.show_hidden = false,
                ("show_hidden", _) => {
                    warning = Some("warning: config contains invalid show_hidden value".to_owned())
                }
                ("sort_field", "unsorted") => config.sort_field = SortField::Unsorted,
                ("sort_field", "name") => config.sort_field = SortField::Name,
                ("sort_field", "size") => config.sort_field = SortField::Size,
                ("sort_field", "type") => config.sort_field = SortField::Type,
                ("sort_field", _) => {
                    warning = Some("warning: config contains invalid sort_field value".to_owned())
                }
                ("sort_direction", "asc") => config.sort_direction = SortDirection::Asc,
                ("sort_direction", "desc") => config.sort_direction = SortDirection::Desc,
                ("sort_direction", _) => {
                    warning =
                        Some("warning: config contains invalid sort_direction value".to_owned())
                }
                _ => {}
            }
        }

        (config, warning)
    }

    fn serialize(&self) -> String {
        format!(
            "show_hidden={}\nsort_field={}\nsort_direction={}\n",
            self.show_hidden,
            self.sort_field.as_str(),
            self.sort_direction.as_str()
        )
    }
}

impl SortField {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Unsorted => Self::Name,
            Self::Name => Self::Size,
            Self::Size => Self::Type,
            Self::Type => Self::Unsorted,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unsorted => "unsorted",
            Self::Name => "name",
            Self::Size => "size",
            Self::Type => "type",
        }
    }
}

impl SortDirection {
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

fn config_path() -> Option<PathBuf> {
    if cfg!(windows) {
        return env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|path| path.join("rll").join("config"));
    }

    if let Some(path) = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(path.join("rll").join("config"));
    }

    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| path.join(".config").join("rll").join("config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_current_tui_behavior() {
        let config = Config::default();

        assert!(!config.show_hidden);
        assert_eq!(config.sort_field, SortField::Unsorted);
        assert_eq!(config.sort_direction, SortDirection::Desc);
    }

    #[test]
    fn parses_valid_config() {
        let (config, warning) =
            Config::parse("show_hidden=true\nsort_field=name\nsort_direction=asc\n");

        assert_eq!(warning, None);
        assert!(config.show_hidden);
        assert_eq!(config.sort_field, SortField::Name);
        assert_eq!(config.sort_direction, SortDirection::Asc);
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let (config, warning) =
            Config::parse("show_hidden=maybe\nsort_field=mtime\nsort_direction=sideways\n");

        assert!(warning.is_some());
        assert_eq!(config, Config::default());
    }

    #[test]
    fn serialize_round_trip() {
        let config = Config {
            show_hidden: true,
            sort_field: SortField::Type,
            sort_direction: SortDirection::Asc,
        };

        let (parsed, warning) = Config::parse(&config.serialize());

        assert_eq!(warning, None);
        assert_eq!(parsed, config);
    }

    #[test]
    fn saves_and_loads_from_path() {
        let dir = env::temp_dir().join(format!(
            "rll-config-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let path = dir.join("config");
        let config = Config {
            show_hidden: true,
            sort_field: SortField::Size,
            sort_direction: SortDirection::Asc,
        };

        config.save_to_path(&path).unwrap();
        let (loaded, warning) = Config::load_from_path(&path);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(warning, None);
        assert_eq!(loaded, config);
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
