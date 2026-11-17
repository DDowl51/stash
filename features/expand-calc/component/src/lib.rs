//! 应用功能 `expand-calc`：把输入里的 `=算式` 换成结果，原来的算式记进上下文。

use std::sync::Arc;

use capture_contracts::{
    CAPTURE_EXPANDERS, ContextEntry, ContextKind, Draft, Expander, ExpanderSpec,
};
use expand_calc_core::expand;
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, NoConfig, async_trait,
    register_component,
};

struct Calc;

#[async_trait]
impl Expander for Calc {
    async fn spec(&self) -> Result<ExpanderSpec, AppError> {
        Ok(ExpanderSpec {
            id: "calc".to_owned(),
        })
    }

    async fn expand(&self, mut draft: Draft) -> Result<Draft, AppError> {
        let (text, done) = expand(&draft.text);
        draft.text = text;
        draft.context.extend(
            done.into_iter()
                .map(|d| ContextEntry::new("calc.expr", "算式", ContextKind::Text, d)),
        );
        Ok(draft)
    }
}

pub struct ExpandCalc;

impl Component for ExpandCalc {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<NoConfig>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "expand-calc",
            kind: Kind::Feature,
            summary: "把输入里的 =算式 换成结果",
            provides: vec![],
            requires: vec![CAPTURE_EXPANDERS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        config.parse::<NoConfig>()?;
        host.contribute(&CAPTURE_EXPANDERS, Arc::new(Calc))
    }
}

register_component!(ExpandCalc);

#[cfg(test)]
mod tests {
    use capture_contracts::testing::expander_contract;

    use super::*;

    #[tokio::test]
    async fn passes_the_expander_contract() {
        expander_contract(Arc::new(Calc), "没有算式 = 的文字")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn replaces_and_remembers() {
        let got = Calc
            .expand(Draft {
                text: "买 3 个 =3*45".to_owned(),
                ..Draft::default()
            })
            .await
            .unwrap();
        assert_eq!(got.text, "买 3 个 135");
        assert_eq!(got.context[0].value, "3*45 = 135");
    }
}
