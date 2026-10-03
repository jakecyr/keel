use super::*;

#[test]
fn root_language_guide_examples_are_checked_and_tested_by_both_engines() {
    let guide = include_str!("../LANGUAGE.md");
    let mut count = 0;
    for block in guide.split("```keel\n").skip(1) {
        let source = block.split("```").next().unwrap();
        let (program, analysis) = checked(source).unwrap();
        let report = test_engine(
            source,
            &program,
            &analysis,
            &["--engine".into(), "both".into()],
        )
        .unwrap();
        assert_eq!(report["status"], "TESTED", "{report}");
        count += 1;
    }
    assert_eq!(count, 6, "keep the runnable language tour covered");
}

#[test]
fn persistent_service_invalidates_by_complete_source_and_evicts() {
    let mut service = service::Service::new(20_000);
    let valid = "fn answer() -> Int { return 42 }";
    let one = service.request(json!({"id":1,"method":"check","source":valid}));
    assert_eq!(one["result"]["status"], "CHECKED");
    assert_eq!(
        service.request(json!({"id":2,"method":"check","source":valid}))["result"]["revision"],
        one["result"]["revision"]
    );
    let changed = service
        .request(json!({"id":3,"method":"check","source":"fn answer() -> Int { return false }"}));
    assert_eq!(changed["result"]["status"], "FAILED");
    let stats = service.request(json!({"method":"stats"}));
    assert_eq!(stats["result"]["hits"], 1);
    assert_eq!(stats["result"]["misses"], 2);
    assert!(stats["result"]["cache_estimated_bytes"].as_u64().unwrap() <= 20_000);
    assert_eq!(stats["result"]["cache_entries"], 1);
    assert_eq!(
        service.request(json!({"method":"check","source":valid}))["result"]["status"],
        "CHECKED"
    );
    assert_eq!(
        service.request(json!({"method":"stats"}))["result"]["misses"],
        3
    );
}

#[test]
fn persistent_service_errors_remain_protocol_messages() {
    let mut service = service::Service::new(1_000_000);
    for request in [
        json!({"id":17,"method":"absent"}),
        json!({"id":17,"method":"check","source":"fn main() {}","path":"missing"}),
        json!({"id":17,"method":"check","source":"fn main() {}","args":["--unknown"]}),
        json!({"id":17,"method":"check","source":"fn main() {}","extra":true}),
    ] {
        let response = service.request(request);
        assert_eq!(response["id"], 17);
        assert_eq!(response["error"]["kind"], "protocol_error");
    }
    assert_eq!(
        service.request(json!({"method":"check","source":"fn main() {}"}))["result"]["status"],
        "CHECKED"
    );
}

#[test]
fn differential_runner_compares_sampled_results_without_upgrading_assurance() {
    for (source, status) in [
        (include_str!("../examples/collections.keel"), "TESTED"),
        (include_str!("../examples/holes.keel"), "BLOCKED"),
        (include_str!("../examples/counterexample.keel"), "FAILED"),
    ] {
        let (program, analysis) = checked(source).unwrap();
        let args = [
            "--engine".into(),
            "both".into(),
            "--cases".into(),
            "40".into(),
        ];
        let result = test_engine(source, &program, &analysis, &args).unwrap();
        assert_eq!(result["status"], status, "{result}");
        assert_eq!(result["differential"]["status"], "TESTED");
        assert_eq!(result["differential"]["mismatches"], json!([]));
    }
}

#[test]
fn project_errors_map_to_physical_files_and_direct_acceptance_is_protected() {
    let temp = Temp::new().unwrap();
    let project_dir = temp.0.join("app");
    project::init(&project_dir).unwrap();
    let entry = project_dir.join("src/main.keel");
    let tests = project_dir.join("tests/acceptance.keel");
    fs::write(&tests, "test \"wrong type\" {\n    assert 17\n}\n").unwrap();
    let project = project::Project::load(&project_dir).unwrap();
    let mut diagnostic = checked(&project.source).err().unwrap();
    project.annotate(&mut diagnostic);
    assert_eq!(
        diagnostic["diagnostics"][0]["file"],
        json!(fs::canonicalize(&tests).unwrap())
    );
    assert_eq!(diagnostic["diagnostics"][0]["line"], 2);
    assert!(project::Project::load(&tests).unwrap().parts[0].protected);
    assert!(!project::Project::load(&entry).unwrap().parts[0].protected);
}

#[test]
fn multiple_body_edits_validate_combined_result_and_commit_one_file() {
    let temp = Temp::new().unwrap();
    let path = temp.0.join("main.keel");
    let request = temp.0.join("edit.json");
    let source = "fn a() -> Int { return 1 }\nfn b() -> Int { return 2 }\ntest \"sum\" { assert a() + b() == 3 }\n";
    fs::write(&path, source).unwrap();
    let (program, _) = checked(source).unwrap();
    let packet = json!({"base_revision":revision(source),"edits":[{"target":"fn:a","operation":"replace_body","source":"{ return 2 }"},{"target":"fn:b","operation":"replace_body","source":"{ return 1 }"}],"run":"affected_checks_and_tests"});
    fs::write(&request, packet.to_string()).unwrap();
    let args = vec!["--request".into(), request.to_string_lossy().into_owned()];
    let result = edit(&path, source, &program, &args).unwrap();
    assert_eq!(result["status"], "APPLIED");
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("fn a() -> Int { return 2 }"));
    assert!(after.ends_with("test \"sum\" { assert a() + b() == 3 }\n"));
    let (p, a) = checked(&after).unwrap();
    let report = run_tests(&after, &p, &a, &test_options(&[]).unwrap()).unwrap();
    assert_eq!(report["status"], "TESTED");
}

#[test]
fn lint_is_actionable_and_can_fail_ci_without_mutating_source() {
    let source = "fn main() effects { io.stdout } { let unused = 1 }";
    let (program, _) = checked(source).unwrap();
    let report = lint::run(source, &program, true);
    assert_eq!(report["status"], "FAILED");
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 2);
    assert_eq!(lint::run(source, &program, false)["status"], "LINTED");
}

#[test]
fn formatter_preserves_string_values_across_indentation_corpus() {
    for width in 0..30 {
        let source = format!(
            "// braces {{}} are a comment\nfn main() effects {{ io.stdout }} {{\n{}io.println(\"escaped \\\" // {{ }}\\nline\\tend\")\n}}\n",
            " ".repeat(width)
        );
        let formatted = format::source(&source);
        assert_eq!(format::source(&formatted), formatted);
        let before = syntax::parse(&source).unwrap();
        let after = syntax::parse(&formatted).unwrap();
        fn literal(program: &Program) -> &str {
            let syntax::StmtKind::Expr(e) = &program.functions[0].body[0].kind else {
                panic!()
            };
            let syntax::ExprKind::Call(_, args) = &e.kind else {
                panic!()
            };
            let syntax::ExprKind::Text(text) = &args[0].kind else {
                panic!()
            };
            text
        }
        assert_eq!(literal(&before), literal(&after));
    }
}

#[test]
fn review_reports_changed_acceptance_body_even_with_same_test_count() {
    let temp = Temp::new().unwrap();
    let baseline = temp.0.join("old.keel");
    fs::write(&baseline, "fn main() {} test \"truth\" { assert true }").unwrap();
    let source = "fn main() {} test \"truth\" { assert false }";
    let (program, _) = checked(source).unwrap();
    let report = review(
        source,
        &program,
        &["--against".into(), baseline.to_string_lossy().into_owned()],
    )
    .unwrap();
    assert_eq!(report["test_changes"][0]["test"], "truth");
    assert_eq!(
        report["test_changes"][0]["acceptance_review_required"],
        true
    );
}
