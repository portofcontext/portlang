/// Integration test for nested schema resolution
///
/// This tests the full pipeline: Python tool extraction + ty resolver
/// for complex nested Pydantic models.
use portlang_config::{PythonToolExtractor, TyResolverHybrid};
use serde_json::Value;
use std::path::Path;

#[test]
fn test_complex_nested_schema_resolution() {
    let source = r#"
from typing import List, Dict, Optional
from pydantic import BaseModel

class Address(BaseModel):
    street: str
    city: str
    state: str
    zipcode: str
    country: str = "USA"

class PhoneNumber(BaseModel):
    number: str
    extension: Optional[str] = None
    type: str = "mobile"

class User(BaseModel):
    name: str
    email: str
    age: int
    addresses: List[Address]
    phone_numbers: List[PhoneNumber]
    metadata: Dict[str, str]
    primary_address: Address

def create_user(user: User) -> User:
    """Create a new user in the system."""
    return user
"#;

    // Create ty resolver
    let mut resolver = TyResolverHybrid::new(Path::new("test.py"), source).unwrap();

    // Resolve the User type
    let user_schema = resolver.resolve_type("User").unwrap();

    // Debug: print the schema
    eprintln!(
        "User schema:\n{}",
        serde_json::to_string_pretty(&user_schema).unwrap()
    );

    // Verify top-level structure
    assert_eq!(
        user_schema.get("type").and_then(|v| v.as_str()),
        Some("object")
    );

    let properties = user_schema
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();

    // Verify simple fields
    assert_eq!(
        properties
            .get("name")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("string")
    );
    assert_eq!(
        properties
            .get("age")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("integer")
    );

    // Verify List[Address] resolution
    let addresses = properties.get("addresses").unwrap();
    assert_eq!(
        addresses.get("type").and_then(|v| v.as_str()),
        Some("array")
    );

    let address_items = addresses.get("items").and_then(|v| v.as_object()).unwrap();
    let address_props = address_items
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();

    // Verify nested Address fields
    assert!(address_props.contains_key("street"));
    assert!(address_props.contains_key("city"));
    assert!(address_props.contains_key("state"));
    assert!(address_props.contains_key("zipcode"));
    assert!(address_props.contains_key("country"));

    // Verify Address required fields (country has default, so not required)
    let address_required = address_items
        .get("required")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(address_required.len(), 4); // street, city, state, zipcode
    assert!(address_required.iter().any(|v| v == "street"));
    assert!(address_required.iter().any(|v| v == "city"));
    assert!(!address_required.iter().any(|v| v == "country")); // Has default

    // Verify List[PhoneNumber] resolution
    let phone_numbers = properties.get("phone_numbers").unwrap();
    assert_eq!(
        phone_numbers.get("type").and_then(|v| v.as_str()),
        Some("array")
    );

    let phone_items = phone_numbers
        .get("items")
        .and_then(|v| v.as_object())
        .unwrap();
    let phone_props = phone_items
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();

    assert!(phone_props.contains_key("number"));
    assert!(phone_props.contains_key("extension"));
    assert!(phone_props.contains_key("type"));

    // Verify Dict[str, str]
    let metadata = properties.get("metadata").unwrap();
    assert_eq!(
        metadata.get("type").and_then(|v| v.as_str()),
        Some("object")
    );
    let additional = metadata.get("additionalProperties").unwrap();
    assert_eq!(
        additional.get("type").and_then(|v| v.as_str()),
        Some("string")
    );

    // Verify direct nested class (primary_address: Address)
    let primary_address = properties.get("primary_address").unwrap();
    assert_eq!(
        primary_address.get("type").and_then(|v| v.as_str()),
        Some("object")
    );
    let primary_props = primary_address
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();
    assert!(primary_props.contains_key("street"));
    assert!(primary_props.contains_key("city"));
}

#[test]
fn test_python_tool_extractor_with_nested_schemas() {
    let source = r#"
from typing import List
from pydantic import BaseModel

class Address(BaseModel):
    street: str
    city: str

class User(BaseModel):
    name: str
    addresses: List[Address]

def create_user(user: User) -> User:
    """Create a new user with addresses."""
    return user
"#;

    let mut extractor = PythonToolExtractor::new().unwrap();

    // Extract the tool
    let tools = extractor.extract_tools_from_source(source).unwrap();

    assert_eq!(tools.len(), 1);
    let tool = &tools[0];

    assert_eq!(tool.name, "create_user");
    assert_eq!(
        tool.description.as_deref(),
        Some("Create a new user with addresses.")
    );

    // Verify the input schema has nested Address resolution
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(|v: &Value| v.as_object())
        .unwrap();
    let user_param = properties.get("user").unwrap();

    // Should have object type with nested properties
    assert_eq!(
        user_param.get("type").and_then(|v: &Value| v.as_str()),
        Some("object")
    );

    let user_props = user_param
        .get("properties")
        .and_then(|v: &Value| v.as_object())
        .unwrap();
    assert!(user_props.contains_key("name"));
    assert!(user_props.contains_key("addresses"));

    // Verify addresses is an array with Address items
    let addresses = user_props.get("addresses").unwrap();
    assert_eq!(
        addresses.get("type").and_then(|v: &Value| v.as_str()),
        Some("array")
    );

    let items = addresses
        .get("items")
        .and_then(|v: &Value| v.as_object())
        .unwrap();
    let item_props = items
        .get("properties")
        .and_then(|v: &Value| v.as_object())
        .unwrap();
    assert!(item_props.contains_key("street"));
    assert!(item_props.contains_key("city"));
}

#[test]
fn test_deeply_nested_schemas() {
    let source = r#"
from typing import List

class Country(BaseModel):
    name: str
    code: str

class City(BaseModel):
    name: str
    country: Country

class Address(BaseModel):
    street: str
    city: City

class User(BaseModel):
    name: str
    addresses: List[Address]

def register_user(user: User) -> User:
    """Register a user with deeply nested location data."""
    return user
"#;

    let mut resolver = TyResolverHybrid::new(Path::new("test.py"), source).unwrap();
    let user_schema = resolver.resolve_type("User").unwrap();

    // Navigate through: User -> addresses: List[Address] -> city: City -> country: Country
    let properties = user_schema
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();
    let addresses = properties.get("addresses").unwrap();
    let address_items = addresses.get("items").and_then(|v| v.as_object()).unwrap();
    let address_props = address_items
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();

    // Check City is nested
    let city = address_props.get("city").unwrap();
    assert_eq!(city.get("type").and_then(|v| v.as_str()), Some("object"));

    let city_props = city.get("properties").and_then(|v| v.as_object()).unwrap();
    assert!(city_props.contains_key("name"));
    assert!(city_props.contains_key("country"));

    // Check Country is nested inside City
    let country = city_props.get("country").unwrap();
    assert_eq!(country.get("type").and_then(|v| v.as_str()), Some("object"));

    let country_props = country
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();
    assert!(country_props.contains_key("name"));
    assert!(country_props.contains_key("code"));
}
