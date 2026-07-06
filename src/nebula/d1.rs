use worker::wasm_bindgen::JsValue;

use super::{Statement, Value};

const MAX_SAFE_JS_INTEGER: i64 = 9_007_199_254_740_991;
const MIN_SAFE_JS_INTEGER: i64 = -9_007_199_254_740_991;

/// Executes statements through D1's `batch()` API.
///
/// D1 executes a batch transactionally: if one statement fails, the batch
/// is aborted/rolled back by D1 and the error is returned by the Worker
/// runtime. Nebula keeps this explicit so ordinary single-statement calls
/// do not accidentally imply transaction boundaries.
pub async fn batch_d1(
    db: &worker::D1Database,
    statements: impl IntoIterator<Item = Statement>,
) -> worker::Result<Vec<worker::D1Result>> {
    let prepared = statements
        .into_iter()
        .map(|statement| statement.prepare_d1(db))
        .collect::<worker::Result<Vec<_>>>()?;

    db.batch(prepared).await
}

impl Statement {
    pub fn bind_js_values(&self) -> worker::Result<Vec<JsValue>> {
        self.binds.iter().map(value_to_js).collect()
    }

    pub fn prepare_d1(
        &self,
        db: &worker::D1Database,
    ) -> worker::Result<worker::D1PreparedStatement> {
        let statement = db.prepare(&self.sql);
        let binds = self.bind_js_values()?;
        statement.bind(&binds)
    }

    pub async fn execute_d1(&self, db: &worker::D1Database) -> worker::Result<worker::D1Result> {
        self.prepare_d1(db)?.run().await
    }

    pub async fn fetch_all_d1(&self, db: &worker::D1Database) -> worker::Result<worker::D1Result> {
        self.prepare_d1(db)?.all().await
    }

    pub async fn fetch_optional_d1<T>(&self, db: &worker::D1Database) -> worker::Result<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.prepare_d1(db)?.first(None).await
    }

    pub async fn fetch_one_d1<T>(&self, db: &worker::D1Database) -> worker::Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.fetch_optional_d1(db).await?.ok_or_else(|| {
            worker::Error::RustError("Nebula query expected one row and returned none".into())
        })
    }
}

fn value_to_js(value: &Value) -> worker::Result<JsValue> {
    match value {
        Value::Null => Ok(JsValue::null()),
        Value::Integer(value) => {
            if !(MIN_SAFE_JS_INTEGER..=MAX_SAFE_JS_INTEGER).contains(value) {
                return Err(worker::Error::RustError(format!(
                    "D1 integer bind exceeds JavaScript safe integer range: {value}"
                )));
            }

            Ok(JsValue::from_f64(*value as f64))
        }
        Value::Real(value) => Ok(JsValue::from_f64(*value)),
        Value::Text(value) => Ok(JsValue::from_str(value)),
        Value::Blob(value) => {
            worker::d1::serde_wasm_bindgen::to_value(value).map_err(worker::Error::from)
        }
        Value::Bool(value) => Ok(JsValue::from_bool(*value)),
    }
}
