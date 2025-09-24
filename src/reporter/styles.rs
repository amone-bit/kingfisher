pub use console::{Style, StyledObject, Term};

#[allow(dead_code)]
pub struct Styles {
    pub style_finding_heading: Style,
    pub style_finding_active_heading: Style,
    pub style_rule: Style,
    pub style_heading: Style,
    pub style_active_creds: Style,
    pub style_match: Style,
    pub style_metadata: Style,
}
impl Styles {
    pub fn new(use_color: bool) -> Self {
        // Trust the `use_color` decision from the caller.
        let styles_enabled = use_color;

        let style_finding_heading = Style::new().bright().white().force_styling(styles_enabled);

        let style_finding_active_heading =
            Style::new().bold().bright().cyan().force_styling(styles_enabled);
        let style_rule = Style::new().bright().bold().blue().force_styling(styles_enabled);
        let style_heading = Style::new().bold().force_styling(styles_enabled);
        let style_match = Style::new().yellow().force_styling(styles_enabled);
        let style_metadata = Style::new().bright().blue().force_styling(styles_enabled);
        let style_active_creds = Style::new().bright().cyan().force_styling(styles_enabled);
        Self {
            style_finding_heading,
            style_finding_active_heading,
            style_rule,
            style_heading,
            style_match,
            style_metadata,
            style_active_creds,
        }
    }
}
