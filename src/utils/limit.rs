use cyder_tools::log::{debug, info};
use once_cell::sync::Lazy;

use crate::controller::proxy::RequestInfo;
use crate::database::limit_strategy::{LimitStrategyDetail, LimitStrategyItem};

pub trait Limiter: Sync + Send {
    fn check_limit_strategy(
        &self,
        strategy: &LimitStrategyDetail,
        request_info: &RequestInfo,
    ) -> Result<(), String>;

    fn check_rate_limit(&self, strategy: &LimitStrategyItem) -> Result<(), String>;

    fn check_fee_limit(&self, strategy: &LimitStrategyItem) -> Result<(), String>;
}

pub struct MemoryLimiter {}

impl MemoryLimiter {
    pub fn new() -> Self {
        MemoryLimiter {}
    }
}

impl MemoryLimiter {
    fn inner_check(
        &self,
        strategy: &LimitStrategyDetail,
        request_info: &RequestInfo,
    ) -> Result<(), String> {
        let main_strategy = &strategy.strategy.main_strategy;
        let provider_id = request_info.provider_id;
        let model_id = match request_info.model_id {
            Some(id) => id,
            None => return Err("model is invalid".to_string()),
        };
        match main_strategy.as_str() {
            "unlimited" => {
                debug!("Unlimited strategy, no rate limiting applied");
                Ok(())
            }
            _ => {
                // check black list
                for item in &strategy.black_list {
                    let resource_type = &item.resource_type;
                    let resource_id = match item.resource_id {
                        Some(id) => id,
                        None => return Err("limit is broken".to_string()),
                    };
                    match resource_type.as_str() {
                        "model" => {
                            if model_id == resource_id {
                                return Err("model is in blacklist".to_string());
                            }
                        }
                        "provider" => {
                            if provider_id == resource_id {
                                return Err("provider is in blacklist".to_string());
                            }
                        }
                        _ => return Err("unknown resource_type".to_string()),
                    }
                }

                if !strategy.white_list.is_empty() {
                    let mut is_pass = false;
                    for item in &strategy.white_list {
                        let resource_type = &item.resource_type;
                        let resource_id = match item.resource_id {
                            Some(id) => id,
                            None => return Err("limit is broken".to_string()),
                        };
                        match resource_type.as_str() {
                            "model" => {
                                if model_id == resource_id {
                                    is_pass = true;
                                }
                            }
                            "provider" => {
                                if provider_id == resource_id {
                                    is_pass = true;
                                }
                            }
                            _ => return Err("unknown resource_type".to_string()),
                        }
                    }
                    if !is_pass {
                        return return Err(
                            "only model or provider in white list can pass".to_string()
                        );
                    }
                }

                Ok(())
            }
        }
    }
}

impl Limiter for MemoryLimiter {
    fn check_limit_strategy(
        &self,
        strategy: &LimitStrategyDetail,
        request_info: &RequestInfo,
    ) -> Result<(), String> {
        let result = self.inner_check(strategy, request_info);
        if let Err(msg) = &result {
            let provide_key = &request_info.provider_key;
            let mode_name = &request_info.model_name;
            let api_key_name = &request_info.api_key_name;
            let strategy_name = &strategy.strategy.name;
            info!("Interrupted: {api_key_name} {provide_key}/{mode_name}, interrupted by {strategy_name}({msg})")
        }
        result
    }

    fn check_rate_limit(&self, _strategy: &LimitStrategyItem) -> Result<(), String> {
        // Implement the logic for checking rate limits here
        Ok(())
    }

    fn check_fee_limit(&self, _strategy: &LimitStrategyItem) -> Result<(), String> {
        // Implement the logic for checking fee limits here
        Ok(())
    }
}

pub static LIMITER: Lazy<Box<dyn Limiter + Sync + Send>> =
    Lazy::new(|| Box::new(MemoryLimiter::new()));
