pub const JSON_SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;
pub const JSON_SAFE_INTEGER_MIN: i64 = -9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "i64", into = "i64")]
#[ts(type = "number")]
pub struct JsonSafeI64(i64);

impl JsonSafeI64 {
    pub fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for JsonSafeI64 {
    type Error = String;

    fn try_from(value: i64) -> std::result::Result<Self, Self::Error> {
        if (JSON_SAFE_INTEGER_MIN..=JSON_SAFE_INTEGER_MAX as i64).contains(&value) {
            Ok(Self(value))
        } else {
            Err(format!(
                "{value} is outside the JSON safe-integer range [{JSON_SAFE_INTEGER_MIN}, {JSON_SAFE_INTEGER_MAX}]"
            ))
        }
    }
}

impl From<JsonSafeI64> for i64 {
    fn from(value: JsonSafeI64) -> Self {
        value.0
    }
}

impl JsonSchema for JsonSafeI64 {
    fn schema_name() -> String {
        "JsonSafeI64".to_string()
    }

    fn json_schema(_generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        safe_integer_schema(JSON_SAFE_INTEGER_MIN as f64, JSON_SAFE_INTEGER_MAX as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "u64", into = "u64")]
#[ts(type = "number")]
pub struct JsonSafeU64(u64);

impl JsonSafeU64 {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for JsonSafeU64 {
    type Error = String;

    fn try_from(value: u64) -> std::result::Result<Self, Self::Error> {
        if value <= JSON_SAFE_INTEGER_MAX {
            Ok(Self(value))
        } else {
            Err(format!(
                "{value} is outside the JSON safe-integer range [0, {JSON_SAFE_INTEGER_MAX}]"
            ))
        }
    }
}

impl TryFrom<usize> for JsonSafeU64 {
    type Error = String;

    fn try_from(value: usize) -> std::result::Result<Self, Self::Error> {
        Self::try_from(value as u64)
    }
}

impl From<JsonSafeU64> for u64 {
    fn from(value: JsonSafeU64) -> Self {
        value.0
    }
}

impl JsonSchema for JsonSafeU64 {
    fn schema_name() -> String {
        "JsonSafeU64".to_string()
    }

    fn json_schema(_generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        safe_integer_schema(0.0, JSON_SAFE_INTEGER_MAX as f64)
    }
}

fn safe_integer_schema(minimum: f64, maximum: f64) -> schemars::schema::Schema {
    schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::Integer.into()),
        number: Some(Box::new(schemars::schema::NumberValidation {
            minimum: Some(minimum),
            maximum: Some(maximum),
            ..Default::default()
        })),
        ..Default::default()
    }
    .into()
}

macro_rules! safe_integer_serde {
    ($module:ident, $option_module:ident, $primitive:ty, $safe:ty) => {
        mod $module {
            use super::*;

            pub fn serialize<S>(value: &$primitive, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let value = <$safe>::try_from(*value).map_err(serde::ser::Error::custom)?;
                value.serialize(serializer)
            }

            pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<$primitive, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                <$primitive>::try_from(<$safe>::deserialize(deserializer)?.get())
                    .map_err(serde::de::Error::custom)
            }
        }

        mod $option_module {
            use super::*;

            pub fn serialize<S>(
                value: &Option<$primitive>,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                value
                    .map(<$safe>::try_from)
                    .transpose()
                    .map_err(serde::ser::Error::custom)?
                    .serialize(serializer)
            }

            pub fn deserialize<'de, D>(
                deserializer: D,
            ) -> std::result::Result<Option<$primitive>, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Option::<$safe>::deserialize(deserializer)?
                    .map(|value| {
                        <$primitive>::try_from(value.get()).map_err(serde::de::Error::custom)
                    })
                    .transpose()
            }
        }
    };
}

safe_integer_serde!(json_safe_i64, option_json_safe_i64, i64, JsonSafeI64);
safe_integer_serde!(json_safe_u64, option_json_safe_u64, u64, JsonSafeU64);
safe_integer_serde!(
    json_safe_usize,
    option_json_safe_usize,
    usize,
    JsonSafeU64
);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    struct SignedBoundary {
        #[serde(with = "json_safe_i64")]
        value: i64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct UnsignedBoundary {
        #[serde(with = "json_safe_u64")]
        value: u64,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(transparent)]
    struct NarrowSafeU64(u64);

    impl NarrowSafeU64 {
        fn get(self) -> u64 {
            self.0
        }
    }

    impl TryFrom<u32> for NarrowSafeU64 {
        type Error = String;

        fn try_from(value: u32) -> std::result::Result<Self, Self::Error> {
            Ok(Self(u64::from(value)))
        }
    }

    safe_integer_serde!(
        json_safe_u32_for_test,
        option_json_safe_u32_for_test,
        u32,
        NarrowSafeU64
    );

    #[derive(Debug, Serialize, Deserialize)]
    struct NarrowBoundary {
        #[serde(with = "json_safe_u32_for_test")]
        value: u32,
        #[serde(with = "option_json_safe_u32_for_test")]
        optional: Option<u32>,
    }

    #[test]
    fn safe_integer_bounds_round_trip() {
        for value in [JSON_SAFE_INTEGER_MIN, 0, JSON_SAFE_INTEGER_MAX as i64] {
            let encoded = serde_json::to_string(&SignedBoundary { value }).expect("serialize");
            let decoded: SignedBoundary = serde_json::from_str(&encoded).expect("deserialize");
            assert_eq!(decoded.value, value);
        }
        let encoded = serde_json::to_string(&UnsignedBoundary {
            value: JSON_SAFE_INTEGER_MAX,
        })
        .expect("serialize");
        let decoded: UnsignedBoundary = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded.value, JSON_SAFE_INTEGER_MAX);
    }

    #[test]
    fn out_of_range_input_and_output_are_rejected() {
        assert!(
            serde_json::from_str::<SignedBoundary>(r#"{"value":9007199254740992}"#).is_err()
        );
        assert!(
            serde_json::to_string(&SignedBoundary {
                value: JSON_SAFE_INTEGER_MIN - 1,
            })
            .is_err()
        );
        assert!(
            serde_json::from_str::<UnsignedBoundary>(r#"{"value":9007199254740992}"#).is_err()
        );
        assert!(
            serde_json::to_string(&UnsignedBoundary {
                value: JSON_SAFE_INTEGER_MAX + 1,
            })
            .is_err()
        );
    }

    #[test]
    fn target_width_overflow_is_rejected_for_values_that_are_json_safe() {
        assert!(
            serde_json::from_str::<NarrowBoundary>(
                r#"{"value":4294967296,"optional":null}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<NarrowBoundary>(
                r#"{"value":0,"optional":4294967296}"#
            )
            .is_err()
        );
    }

    #[test]
    fn representative_typescript_declarations_are_camel_case_numbers() {
        let activity = GatewayActivityView::decl();
        assert!(activity.contains("activeTurnId"));
        assert!(!activity.contains("active_turn_id"));
        assert!(!activity.contains("bigint"));

        let usage = SessionUsageSummaryView::decl();
        assert!(usage.contains("estimatedCostNanodollars: number"));
        assert!(!usage.contains("bigint"));
    }

    #[test]
    fn opaque_inode_round_trips_as_an_exact_decimal_string() {
        let state: ManagedServerState = serde_json::from_value(serde_json::json!({
            "instanceId": null,
            "pid": 7,
            "baseUrl": "http://127.0.0.1:1",
            "readyzUrl": "http://127.0.0.1:1/readyz",
            "startedAtMs": 1,
            "version": "test",
            "executablePath": "/tmp/pevo",
            "executableModifiedMs": 1,
            "executableSize": 1,
            "executableInode": "18446744073709551615",
            "staticDir": null
        }))
        .expect("managed state");
        assert_eq!(
            state.executable_inode.as_deref(),
            Some("18446744073709551615")
        );
        assert_eq!(
            serde_json::to_value(state).expect("serialize")["executableInode"],
            "18446744073709551615"
        );
    }
}
