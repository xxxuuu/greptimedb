// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt;

use common_error::ext::BoxedError;
use common_macro::admin_fn;
use common_query::error::Error::ThreadJoin;
use common_query::error::{
    InvalidFuncArgsSnafu, MissingTableMutationHandlerSnafu, Result, TableMutationSnafu,
    UnsupportedInputDataTypeSnafu,
};
use common_query::prelude::{Signature, Volatility};
use common_telemetry::error;
use datatypes::prelude::*;
use datatypes::vectors::VectorRef;
use session::context::QueryContextRef;
use session::table_name::table_name_to_full_name;
use snafu::{ensure, Location, OptionExt, ResultExt};
use table::requests::{CompactTableRequest, FlushTableRequest};

use crate::ensure_greptime;
use crate::function::{Function, FunctionContext};
use crate::handlers::TableMutationHandlerRef;

macro_rules! define_table_function {
    ($name: expr, $display_name_str: expr, $display_name: ident, $func: ident, $request: ident) => {
        /// A function to $func table, such as `$display_name(table_name)`.
        #[admin_fn(name = $name, display_name = $display_name_str, sig_fn = "signature", ret = "uint64")]
        pub(crate) async fn $display_name(
            table_mutation_handler: &TableMutationHandlerRef,
            query_ctx: &QueryContextRef,
            params: &[ValueRef<'_>],
        ) -> Result<Value> {
            ensure!(
                params.len() == 1,
                InvalidFuncArgsSnafu {
                    err_msg: format!(
                        "The length of the args is not correct, expect 1, have: {}",
                        params.len()
                    ),
                }
            );

            let ValueRef::String(table_name) = params[0] else {
                return UnsupportedInputDataTypeSnafu {
                    function: $display_name_str,
                    datatypes: params.iter().map(|v| v.data_type()).collect::<Vec<_>>(),
                }
                .fail();
            };

            let (catalog_name, schema_name, table_name) =
                table_name_to_full_name(table_name, &query_ctx)
                    .map_err(BoxedError::new)
                    .context(TableMutationSnafu)?;

            let affected_rows = table_mutation_handler
                .$func(
                    $request {
                        catalog_name,
                        schema_name,
                        table_name,
                    },
                    query_ctx.clone(),
                )
                .await?;

            Ok(Value::from(affected_rows as u64))
        }
    };
}

define_table_function!(
    "FlushTableFunction",
    "flush_table",
    flush_table,
    flush,
    FlushTableRequest
);

define_table_function!(
    "CompactTableFunction",
    "compact_table",
    compact_table,
    compact,
    CompactTableRequest
);

fn signature() -> Signature {
    Signature::uniform(
        1,
        vec![ConcreteDataType::string_datatype()],
        Volatility::Immutable,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use common_query::prelude::TypeSignature;
    use datatypes::vectors::{StringVector, UInt64Vector};

    use super::*;

    macro_rules! define_table_function_test {
        ($name: ident, $func: ident) => {
            paste::paste!{
                #[test]
                fn [<test_ $name _misc>]() {
                    let f = $func;
                    assert_eq!(stringify!($name), f.name());
                    assert_eq!(
                        ConcreteDataType::uint64_datatype(),
                        f.return_type(&[]).unwrap()
                    );
                    assert!(matches!(f.signature(),
                                     Signature {
                                         type_signature: TypeSignature::Uniform(1, valid_types),
                                         volatility: Volatility::Immutable
                                     } if valid_types == vec![ConcreteDataType::string_datatype()]));
                }

                #[test]
                fn [<test_ $name _missing_table_mutation>]() {
                    let f = $func;

                    let args = vec!["test"];

                    let args = args
                        .into_iter()
                        .map(|arg| Arc::new(StringVector::from(vec![arg])) as _)
                        .collect::<Vec<_>>();

                    let result = f.eval(FunctionContext::default(), &args).unwrap_err();
                    assert_eq!(
                        "Missing TableMutationHandler, not expected",
                        result.to_string()
                    );
                }

                #[test]
                fn [<test_ $name>]() {
                    let f = $func;


                    let args = vec!["test"];

                    let args = args
                        .into_iter()
                        .map(|arg| Arc::new(StringVector::from(vec![arg])) as _)
                        .collect::<Vec<_>>();

                    let result = f.eval(FunctionContext::mock(), &args).unwrap();

                    let expect: VectorRef = Arc::new(UInt64Vector::from_slice([42]));
                    assert_eq!(expect, result);
                }
            }
        }
    }

    define_table_function_test!(flush_table, FlushTableFunction);

    define_table_function_test!(compact_table, CompactTableFunction);
}
