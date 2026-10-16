use super::{
    ExecutionLimits, HttpResponse, memory_stores, route_response, route_response_with_body,
    route_response_with_stores, route_table,
};

#[test]
fn routes_get_requests_to_orni_string_handlers() {
    let program = crate::compile(
        r#"
            func hello(name: string) -> string { return "Hello, " + name + "!"; }
            route get "/hello/:name" => hello;
        "#,
    )
    .expect("the example route should compile");
    let routes = route_table(&program);

    assert_eq!(
        route_response(
            "GET",
            "/hello/Orni",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (200, "Hello, Orni!".to_owned())
    );
    assert_eq!(
        route_response(
            "GET",
            "/hello/Orni?locale=en",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (
            400,
            r#"{"error":{"code":"ORNI_HTTP_INVALID_QUERY_PARAMETER","message":"Unexpected query parameter 'locale'","status":400}}"#.to_owned(),
        )
    );
    assert_eq!(
        route_response(
            "GET",
            "/missing",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (
            404,
            r#"{"error":{"code":"ORNI_HTTP_ROUTE_NOT_FOUND","message":"Route not found","status":404}}"#.to_owned(),
        )
    );
    assert_eq!(
        route_response(
            "POST",
            "/hello/Orni",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (
            405,
            r#"{"error":{"code":"ORNI_HTTP_METHOD_NOT_ALLOWED","message":"Method Not Allowed","status":405}}"#.to_owned(),
        )
    );
}

#[test]
fn get_routes_bind_typed_query_parameters_and_defaults() {
    let program = crate::compile(
        r#"
            func users(page: uint16 = 1, active: bool = true) -> response {
                if active { return response.ok("active-" + page); }
                return response.ok("inactive-" + page);
            }
            func search(term: string) -> response { return response.ok(term); }
            route get "/users" => users;
            route get "/search" => search;
        "#,
    )
    .expect("the query route example should compile");
    let routes = route_table(&program);
    let limits = ExecutionLimits::default();

    assert_eq!(
        route_response("GET", "/users", &program, &routes, limits),
        (200, "active-1".to_owned())
    );
    assert_eq!(
        route_response(
            "GET",
            "/users?page=25&active=false",
            &program,
            &routes,
            limits
        ),
        (200, "inactive-25".to_owned())
    );
    assert_eq!(
        route_response(
            "GET",
            "/search?term=Orni%20Framework",
            &program,
            &routes,
            limits
        ),
        (200, "Orni Framework".to_owned())
    );

    let invalid =
        route_response_with_body("GET", "/users?page=70000", "", &program, &routes, limits);
    assert_eq!(invalid.status, 400);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&invalid.body)
            .expect("error JSON")
            .pointer("/error/code"),
        Some(&serde_json::json!("ORNI_HTTP_INVALID_QUERY_PARAMETER"))
    );

    let missing = route_response_with_body("GET", "/search", "", &program, &routes, limits);
    assert_eq!(missing.status, 400);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&missing.body)
            .expect("error JSON")
            .pointer("/error/message"),
        Some(&serde_json::json!(
            "Missing required query parameter 'term'"
        ))
    );
}

#[test]
fn serializes_standard_errors_as_json_with_a_stable_code() {
    let program = crate::compile(
        r#"
            func user() -> response { return response.not_found("User not found"); }
            route get "/users/current" => user;
        "#,
    )
    .expect("the error response route should compile");
    let routes = route_table(&program);

    let missing = route_response_with_body(
        "GET",
        "/missing",
        "",
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(missing.status, 404);
    assert_eq!(missing.content_type, "application/json; charset=utf-8");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&missing.body)
            .expect("framework errors should be JSON")
            .pointer("/error/code"),
        Some(&serde_json::Value::String(
            "ORNI_HTTP_ROUTE_NOT_FOUND".to_owned()
        ))
    );

    let method = route_response_with_body(
        "POST",
        "/users/current",
        "",
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(method.status, 405);
    assert_eq!(method.headers, vec![("Allow".to_owned(), "GET".to_owned())]);

    let application = route_response_with_body(
        "GET",
        "/users/current",
        "",
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(application.status, 404);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&application.body)
            .expect("application errors should be JSON")
            .pointer("/error/code"),
        Some(&serde_json::Value::String(
            "ORNI_HTTP_RESOURCE_NOT_FOUND".to_owned()
        ))
    );
}

#[test]
fn validates_request_bodies_with_field_details() {
    let program = crate::compile(
        r#"
            model CreateUser { name: string<4>; }
            func create(input: CreateUser) -> response { return response.created(input); }
            route post "/users" => create;
        "#,
    )
    .expect("the validation route should compile");
    let response = route_response_with_body(
        "POST",
        "/users",
        r#"{"name":"Too long"}"#,
        &program,
        &route_table(&program),
        ExecutionLimits::default(),
    );

    assert_eq!(response.status, 400);
    let error = serde_json::from_str::<serde_json::Value>(&response.body)
        .expect("validation errors should be JSON");
    assert_eq!(
        error.pointer("/error/code"),
        Some(&serde_json::json!("ORNI_HTTP_INVALID_BODY"))
    );
    assert_eq!(
        error.pointer("/error/details/name"),
        Some(&serde_json::json!(
            "Invalid JSON field 'name': expected string<4>"
        )),
    );
}

#[test]
fn serializes_explicit_standard_status_helpers() {
    let program = crate::compile(
        r#"
            func unauthenticated() -> response { return response.unauthorized("Sign in first"); }
            func denied() -> response { return response.forbidden("Not allowed"); }
            func duplicate() -> response { return response.conflict("Already exists"); }
            route get "/unauthenticated" => unauthenticated;
            route get "/denied" => denied;
            route get "/duplicate" => duplicate;
        "#,
    )
    .expect("the status helper routes should compile");
    let routes = route_table(&program);
    for (path, status, code) in [
        ("/unauthenticated", 401, "ORNI_HTTP_UNAUTHORIZED"),
        ("/denied", 403, "ORNI_HTTP_FORBIDDEN"),
        ("/duplicate", 409, "ORNI_HTTP_CONFLICT"),
    ] {
        let response = route_response_with_body(
            "GET",
            path,
            "",
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(response.status, status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response.body)
                .expect("explicit errors should be JSON")
                .pointer("/error/code"),
            Some(&serde_json::json!(code)),
        );
    }
}

#[test]
fn redirects_to_a_documentation_url() {
    let program = crate::compile(
        r#"
            func home() -> response {
                return response.redirect("https://example.com/docs");
            }
            route get "/" => home;
        "#,
    )
    .expect("the redirect route should compile");
    let response = route_response_with_body(
        "GET",
        "/",
        "",
        &program,
        &route_table(&program),
        ExecutionLimits::default(),
    );

    assert_eq!(response.status, 302);
    assert_eq!(
        response.headers,
        vec![("Location".to_owned(), "https://example.com/docs".to_owned())]
    );
    assert_eq!(response.body, "Redirecting to documentation.");
}

#[test]
fn prefers_literal_routes_over_parameterized_routes() {
    let program = crate::compile(
        r#"
            func settings() -> string { return "Settings"; }
            func profile(name: string) -> string { return name; }
            route get "/users/settings" => settings;
            route get "/users/:name" => profile;
        "#,
    )
    .expect("literal and parameterized routes should coexist");
    let routes = route_table(&program);

    assert_eq!(
        route_response(
            "GET",
            "/users/settings",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (200, "Settings".to_owned())
    );
}

#[test]
fn converts_path_parameters_to_the_handler_types() {
    let program = crate::compile(
        r#"
            func user(id: uint16) -> string { return "User #" + id; }
            route get "/users/:id" => user;
        "#,
    )
    .expect("typed route parameters should compile");
    let routes = route_table(&program);

    assert_eq!(
        route_response(
            "GET",
            "/users/42",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (200, "User #42".to_owned())
    );
    assert_eq!(
        route_response(
            "GET",
            "/users/70000",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (
            400,
            r#"{"error":{"code":"ORNI_HTTP_INVALID_PATH_PARAMETER","message":"Invalid route parameter 'id': expected uint16","status":400}}"#.to_owned()
        )
    );
}

#[test]
fn converts_uuid_path_parameters_before_calling_handlers() {
    let program = crate::compile(
        r#"
            func user(id: uuid) -> string { return "User #" + id; }
            route get "/users/:id" => user;
        "#,
    )
    .expect("UUID route parameters should compile");
    let routes = route_table(&program);
    let uuid = "550e8400-e29b-41d4-a716-446655440000";

    assert_eq!(
        route_response(
            "GET",
            "/users/550e8400-e29b-41d4-a716-446655440000",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (200, format!("User #{uuid}"))
    );
    assert_eq!(
        route_response(
            "GET",
            "/users/not-a-uuid",
            &program,
            &routes,
            ExecutionLimits::default()
        ),
        (
            400,
            r#"{"error":{"code":"ORNI_HTTP_INVALID_PATH_PARAMETER","message":"Invalid route parameter 'id': expected uuid","status":400}}"#.to_owned()
        )
    );
}

#[test]
fn validates_json_models_and_returns_created_resources() {
    let program = crate::compile(
        r#"
            model CreateUser {
                id: uuid;
                name: string<5>;
                email: string<254>;
                active: bool;
            }
            func create_user(input: CreateUser) -> response {
                return response.created(input);
            }
            route post "/users" => create_user;
        "#,
    )
    .expect("a POST route with a model body should compile");
    let routes = route_table(&program);

    let created = route_response_with_body(
        "POST",
        "/users",
        r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","email":"hello@orni.dev","active":true}"#,
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(created.status, 201);
    assert_eq!(created.content_type, "application/json; charset=utf-8");
    assert_eq!(
        created.body,
        r#"{"active":true,"email":"hello@orni.dev","id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni"}"#
    );

    let missing = route_response_with_body(
        "POST",
        "/users",
        r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","active":true}"#,
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(missing.status, 400);
    assert!(missing.body.contains("missing required JSON field 'email'"));

    let too_long = route_response_with_body(
        "POST",
        "/users",
        r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Too long","email":"hello@orni.dev","active":true}"#,
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(too_long.status, 400);
    assert!(too_long.body.contains("expected string<5>"));

    let extra = route_response_with_body(
        "POST",
        "/users",
        r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","email":"hello@orni.dev","active":true,"role":"admin"}"#,
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(extra.status, 400);
    assert!(extra.body.contains("unexpected JSON field 'role'"));

    let invalid_uuid = route_response_with_body(
        "POST",
        "/users",
        r#"{"id":"not-a-uuid","name":"Orni","email":"hello@orni.dev","active":true}"#,
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(invalid_uuid.status, 400);
    assert!(invalid_uuid.body.contains("expected uuid"));

    let wrong_type = route_response_with_body(
        "POST",
        "/users",
        r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","email":"hello@orni.dev","active":"true"}"#,
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(wrong_type.status, 400);
    assert!(wrong_type.body.contains("expected bool"));

    let wrong_method = route_response_with_body(
        "GET",
        "/users",
        "",
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(wrong_method.status, 405);
}

#[test]
fn returns_an_explicit_ok_response_from_a_get_route() {
    let program = crate::compile(
        r#"
            func health() -> response { return response.ok("ready"); }
            route get "/health" => health;
        "#,
    )
    .expect("an explicit GET response should compile");
    let routes = route_table(&program);

    let response = route_response_with_body(
        "GET",
        "/health",
        "",
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(response, HttpResponse::text(200, "ready"));
}

#[test]
fn preserves_custom_headers_from_a_get_response() {
    let program = crate::compile(
        r#"
            func health() -> response {
                return response.header(response.ok("ready"), "Cache-Control", "no-store");
            }
            route get "/health" => health;
        "#,
    )
    .expect("a response with a custom header should compile");
    let routes = route_table(&program);

    let response = route_response_with_body(
        "GET",
        "/health",
        "",
        &program,
        &routes,
        ExecutionLimits::default(),
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.body, "ready");
    assert_eq!(
        response.headers,
        vec![("Cache-Control".to_owned(), "no-store".to_owned())]
    );
}

#[test]
fn paginates_store_lists_with_typed_get_query_parameters() {
    let program = crate::compile(
        r#"
            model User { id: uuid; name: string<80>; }
            model CreateUser { name: string<80>; }
            store users: User;
            func list(page: uint32 = 1, limit: uint16 = 20, sort: string = "name", direction: string = "asc") -> response {
                return users.list(page, limit, sort, direction);
            }
            func create(input: CreateUser) -> response { return users.create(input); }
            route get "/users" => list;
            route post "/users" => create;
        "#,
    )
    .expect("a paginated CRUD application should compile");
    let routes = route_table(&program);
    let stores = memory_stores(&program);
    let limits = ExecutionLimits::default();
    for name in ["Ari", "Bea", "Cam"] {
        let created = route_response_with_stores(
            "POST",
            "/users",
            &format!(r#"{{"name":"{name}"}}"#),
            &program,
            &routes,
            stores.clone(),
            limits,
        );
        assert_eq!(created.status, 201);
    }

    let page = route_response_with_stores(
        "GET",
        "/users?page=2&limit=1",
        "",
        &program,
        &routes,
        stores,
        limits,
    );
    assert_eq!(page.status, 200);
    let json: serde_json::Value = serde_json::from_str(&page.body).expect("page should be JSON");
    assert_eq!(json["page"], 2);
    assert_eq!(json["limit"], 1);
    assert_eq!(json["total"], 3);
    assert_eq!(json["total_pages"], 3);
    assert_eq!(json["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["data"][0]["name"], "Bea");
}

#[test]
fn keeps_crud_models_in_memory_for_one_server_lifetime() {
    let program = crate::compile(
        r#"
            model User { id: uuid; name: string<80>; }
            model CreateUser { name: string<80>; }
            model UpdateUser { name: string<80>; }
            model PatchUser { name: string<80>; }
            store users: User;
            func list() -> response { return users.list(); }
            func create(input: CreateUser) -> response { return users.create(input); }
            func find(id: uuid) -> response { return users.find(id); }
            func replace(id: uuid, input: UpdateUser) -> response { return users.replace(id, input); }
            func patch_user(id: uuid, input: PatchUser) -> response { return users.patch(id, input); }
            func remove(id: uuid) -> response { return users.delete(id); }
            route post "/users" => create;
            route get "/users" => list;
            route get "/users/:id" => find;
            route put "/users/:id" => replace;
            route patch "/users/:id" => patch_user;
            route delete "/users/:id" => remove;
        "#,
    )
    .expect("a CRUD application should compile");
    let routes = route_table(&program);
    let stores = memory_stores(&program);
    let created = route_response_with_stores(
        "POST",
        "/users",
        r#"{"name":"Orni"}"#,
        &program,
        &routes,
        stores.clone(),
        ExecutionLimits::default(),
    );
    assert_eq!(created.status, 201);
    let id = serde_json::from_str::<serde_json::Value>(&created.body)
        .expect("created JSON should be valid")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("created JSON should contain a UUID")
        .to_owned();

    let listed = route_response_with_stores(
        "GET",
        "/users",
        "",
        &program,
        &routes,
        stores.clone(),
        ExecutionLimits::default(),
    );
    assert_eq!(listed.status, 200);
    assert!(listed.body.starts_with('['));
    assert!(listed.body.contains("\"name\":\"Orni\""));

    let found = route_response_with_stores(
        "GET",
        &format!("/users/{id}"),
        "",
        &program,
        &routes,
        stores.clone(),
        ExecutionLimits::default(),
    );
    assert_eq!(found.status, 200);
    assert!(found.body.contains("\"name\":\"Orni\""));

    let updated = route_response_with_stores(
        "PUT",
        &format!("/users/{id}"),
        r#"{"name":"Orni Updated"}"#,
        &program,
        &routes,
        stores.clone(),
        ExecutionLimits::default(),
    );
    assert_eq!(updated.status, 200);
    assert!(updated.body.contains("\"name\":\"Orni Updated\""));

    let patched = route_response_with_stores(
        "PATCH",
        &format!("/users/{id}"),
        r#"{"name":"Orni Patched"}"#,
        &program,
        &routes,
        stores.clone(),
        ExecutionLimits::default(),
    );
    assert_eq!(patched.status, 200);
    assert!(patched.body.contains("\"name\":\"Orni Patched\""));

    let deleted = route_response_with_stores(
        "DELETE",
        &format!("/users/{id}"),
        "",
        &program,
        &routes,
        stores.clone(),
        ExecutionLimits::default(),
    );
    assert_eq!(deleted.status, 204);
    assert!(deleted.body.is_empty());

    let missing = route_response_with_stores(
        "GET",
        &format!("/users/{id}"),
        "",
        &program,
        &routes,
        stores,
        ExecutionLimits::default(),
    );
    assert_eq!(missing.status, 404);
}
