use proptest::{strategy::Strategy, string::string_regex};
use proptest_attributes::proptest;

#[derive(Debug, Clone, PartialEq)]
struct FunctionArn(String);

fn gen_function_arn() -> impl Strategy<Value = FunctionArn> {
    let expr = "arn:aws:lambda:us-east-1:[0-9]{12}:function:custom-runtime";
    let arn = string_regex(expr).unwrap();
    arn.prop_map(FunctionArn)
}

struct SomeStruct;

impl SomeStruct {
    #[proptest(gen_function_arn())]
    fn function_arn(&self) {
        let mut map = HashMap::new();
        map.insert("arn", arn.clone());
        prop_assert_eq!(map.get("arn"), Some(&arn));
    }
}

fn main() {}
