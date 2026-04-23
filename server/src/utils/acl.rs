use crate::schema::enum_def::{Action, RuleScope};
use crate::service::cache::types::CacheApiKeyAclRule;

pub struct AclEvaluator;

impl AclEvaluator {
    pub fn authorize(
        &self,
        label: &str,
        default_action: &Action,
        rules: &[CacheApiKeyAclRule],
        provider_id: i64,
        model_id: i64,
    ) -> Result<(), String> {
        for rule in rules.iter().filter(|rule| rule.is_enabled) {
            let rule_matches = match rule.scope {
                RuleScope::Model => match rule.model_id {
                    Some(rule_model_id) => rule_model_id == model_id,
                    None => {
                        crate::debug_event!(
                            "acl.rule_misconfigured",
                            rule_id = rule.id,
                            scope = "model",
                            missing = "model_id",
                        );
                        false
                    }
                },
                RuleScope::Provider => match rule.provider_id {
                    Some(rule_provider_id) => rule_provider_id == provider_id,
                    None => {
                        crate::debug_event!(
                            "acl.rule_misconfigured",
                            rule_id = rule.id,
                            scope = "provider",
                            missing = "provider_id",
                        );
                        false
                    }
                },
            };

            if !rule_matches {
                continue;
            }

            match rule.effect {
                Action::Allow => return Ok(()),
                Action::Deny => {
                    crate::debug_event!(
                        "acl.request_denied",
                        rule_id = rule.id,
                        scope = format!("{:?}", rule.scope),
                        api_key_label = label,
                        provider_id = provider_id,
                        model_id = model_id,
                    );
                    return Err(format!(
                        "request denied by ACL rule (ID: {}, ApiKey: '{}', Scope: {:?}, Type: {:?})",
                        rule.id, label, rule.scope, rule.effect
                    ));
                }
            }
        }

        match default_action {
            Action::Allow => Ok(()),
            Action::Deny => {
                crate::debug_event!(
                    "acl.default_deny",
                    api_key_label = label,
                    provider_id = provider_id,
                    model_id = model_id,
                );
                Err(format!(
                    "request denied by default api key ACL action from '{}'",
                    label
                ))
            }
        }
    }
}

pub static ACL_EVALUATOR: AclEvaluator = AclEvaluator;

#[cfg(test)]
mod tests {
    use super::AclEvaluator;
    use crate::schema::enum_def::{Action, RuleScope};
    use crate::service::cache::types::CacheApiKeyAclRule;

    fn rule(
        id: i64,
        effect: Action,
        scope: RuleScope,
        provider_id: Option<i64>,
        model_id: Option<i64>,
        priority: i32,
        is_enabled: bool,
    ) -> CacheApiKeyAclRule {
        CacheApiKeyAclRule {
            id,
            effect,
            priority,
            scope,
            provider_id,
            model_id,
            is_enabled,
            description: None,
        }
    }

    #[test]
    fn disabled_rules_do_not_participate_in_authorization() {
        let evaluator = AclEvaluator;
        let rules = vec![
            rule(
                1,
                Action::Deny,
                RuleScope::Model,
                Some(1),
                Some(11),
                1,
                false,
            ),
            rule(
                2,
                Action::Allow,
                RuleScope::Model,
                Some(1),
                Some(11),
                2,
                true,
            ),
        ];

        let result = evaluator.authorize("test", &Action::Deny, &rules, 1, 11);

        assert!(result.is_ok());
    }

    #[test]
    fn provider_rule_applies_to_all_models_under_that_provider() {
        let evaluator = AclEvaluator;
        let rules = vec![rule(
            1,
            Action::Deny,
            RuleScope::Provider,
            Some(1),
            None,
            1,
            true,
        )];

        let result = evaluator.authorize("test", &Action::Allow, &rules, 1, 999);

        assert!(result.is_err());
    }

    #[test]
    fn model_rule_overrides_default_for_matching_model_only() {
        let evaluator = AclEvaluator;
        let rules = vec![rule(
            1,
            Action::Allow,
            RuleScope::Model,
            Some(1),
            Some(11),
            1,
            true,
        )];

        assert!(
            evaluator
                .authorize("test", &Action::Deny, &rules, 1, 11)
                .is_ok()
        );
        assert!(
            evaluator
                .authorize("test", &Action::Deny, &rules, 1, 12)
                .is_err()
        );
    }

    #[test]
    fn invalid_scoped_ids_do_not_match_and_fall_back_to_default() {
        let evaluator = AclEvaluator;
        let rules = vec![
            rule(
                1,
                Action::Allow,
                RuleScope::Provider,
                Some(9),
                Some(11),
                1,
                true,
            ),
            rule(2, Action::Allow, RuleScope::Model, Some(1), None, 2, true),
        ];

        let result = evaluator.authorize("test", &Action::Deny, &rules, 1, 11);

        assert!(result.is_err());
    }
}
