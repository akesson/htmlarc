use crate::*;

#[derive(Clone)]
pub struct ProbeExpression<'s> {
    pub(crate) selector: SelectorList<'s>,
    pub(crate) format: ElementFormat<'s>,
}

impl<'s> TryFrom<&'s str> for ProbeExpression<'s> {
    type Error = Error;

    fn try_from(value: &'s str) -> Result<Self> {
        let (css, format) = value.split_once("=>").ok_or(anyhow!("missing '=>'"))?;

        let selector = parse_css(css.trim()).map_err(|e| anyhow!(e.to_string()))?;
        let format = ElementFormat::try_from(format.trim()).map_err(|e| anyhow!(e.to_string()))?;

        Ok(Self { selector, format })
    }
}

impl ProbeExpression<'_> {
    #[cfg(test)]
    fn string(&self) -> String {
        format!("{} => {}", self.selector, self.format)
    }
}

#[test]
fn parse_probe_expression() {
    let expr = "section > h1 => HtmlFmt[id][class^=mw]";
    let probe = ProbeExpression::try_from(expr).unwrap();

    assert_eq!(probe.string(), expr);

    let expr = "section -> HtmlFmt[id][class^=mw]";
    let probe = ProbeExpression::try_from(expr);
    assert!(probe.is_err());
}
