// Argument parsing and the option model shared by the CLI and TUI dispatch.

#[derive(Clone, Copy, Default)]
pub(crate) enum Mode {
    #[default]
    Cli,
    Tui,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Options {
    pub(crate) order: Option<SortOrder>,
    // Include dotfile entries in top-level listing and recursive sizing.
    pub(crate) show_all: bool,
    // Cap the number of rows printed after sorting/collection.
    pub(crate) top_n: Option<usize>,
    // Emit NDJSON lines instead of the human table.
    pub(crate) json: bool,
    // Subcommand routing; default is the CLI listing path.
    pub(crate) mode: Mode,
}

impl Options {
    pub(crate) fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter().peekable();

        // First positional token before any flag may select the TUI mode.
        if let Some(first) = args.peek() {
            if first.as_ref() == "tui" {
                options.mode = Mode::Tui;
                let _ = args.next();
            }
        }

        while let Some(arg) = args.next() {
            match arg.as_ref() {
                "--o" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--o requires asc or desc".to_owned())?;
                    options.order = Some(SortOrder::parse(value.as_ref())?);
                }
                "--a" | "--all" => {
                    options.show_all = true;
                }
                "--n" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--n requires a positive integer".to_owned())?;
                    let parsed: usize = value
                        .as_ref()
                        .parse()
                        .map_err(|_| "--n requires a positive integer".to_owned())?;
                    if parsed == 0 {
                        return Err("--n requires a positive integer".to_owned());
                    }
                    options.top_n = Some(parsed);
                }
                "--json" => {
                    options.json = true;
                }
                unknown => return Err(format!("unknown option: {unknown}")),
            }
        }

        Ok(options)
    }

    // Stream rows as scanned when no sort or top-N limit is requested; otherwise
    // buffer everything so we can order and truncate before emitting.
    pub(crate) fn buffer_rows(&self) -> bool {
        self.order.is_some() || self.top_n.is_some()
    }

    // Effective sort order. `--n` without explicit `--o` implies `desc` so the
    // "top N biggest" reading matches what callers expect from `head` over `du`.
    // Without this, truncation could drop the directory rows that arrive last
    // from the recursive scan, leaving the printed table inconsistent with TOTAL.
    pub(crate) fn effective_order(&self) -> Option<SortOrder> {
        match (self.order, self.top_n) {
            (Some(order), _) => Some(order),
            (None, Some(_)) => Some(SortOrder::Desc),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            _ => Err("--o requires asc or desc".to_owned()),
        }
    }
}
