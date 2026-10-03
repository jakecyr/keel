use std::collections::BTreeSet;
pub fn validate(args:&[String])->Result<(),String> {
    let command=args.first().ok_or("expected command")?;
    let (flags,values):(&[&str],&[&str])=match command.as_str() {
        "check"=>( &["--json"], &[]),
        "lint"=>( &["--json","--deny-warnings"], &[]),
        "inspect"=>( &["--json"], &["--symbol","--max-chars"]),
        "build"=>( &["--json"], &["-o","--emit-c"]),
        "run"=>( &["--json","--allow-stdout"], &["-o","--emit-c","--policy"]),
        "test"=>( &["--json","--no-shrink"], &["--cases","--seed","--filter","--value","--timeout-ms","--budget-ms","--memory-mib","--engine"]),
        "edit"=>( &["--json","--no-shrink"], &["--request","--cases","--seed","--filter","--value","--timeout-ms","--budget-ms","--memory-mib"]),
        "review"=>( &["--json"], &["--against"]),
        "explain"=>( &["--json"], &["--offset"]),
        "fmt"=>( &["--json","--check"], &[]),
        "init"=>( &["--json"], &[]),
        _=>return Err(format!("unknown command '{command}'; use keel --help")),
    };
    if args.get(1).is_none_or(|s|s.starts_with('-')) {return Err("expected source/project path".into());}
    let mut i=2; let mut seen=BTreeSet::new();
    while i<args.len() {
        let key=if command=="run" && args[i].starts_with("--allow-net=") {"--allow-net"}else{args[i].as_str()};
        if !seen.insert(key.to_string()) {return Err(format!("duplicate option '{key}'"));}
        if flags.contains(&key) || key=="--allow-net" {i+=1;}
        else if values.contains(&key) {
            if args.get(i+1).is_none_or(|v|v.starts_with("--")) {return Err(format!("{key} needs a value"));}
            i+=2;
        } else {return Err(format!("unknown option or extra argument '{}'",args[i]));}
    }
    if seen.contains("--policy") && (seen.contains("--allow-net") || seen.contains("--allow-stdout")) {return Err("--policy cannot be combined with permission overrides".into());}
    Ok(())
}
