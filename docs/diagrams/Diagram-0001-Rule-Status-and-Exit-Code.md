# Rule Status and Exit Code

```mermaid
flowchart TD
    subgraph status["Rule status per rule and account (decided by yata-engine, first match wins)"]
        start["For each rule and account. Groups = account x region x declared type (target_type and related_types; global types use region global). Each group is collected (ResourceSet), failed (CollectionError) or not collected"] --> c1{"Step 1: Any declared type has a group that is not collected?"}
        c1 -->|yes| sk1["skipped (not_collected). Not evaluated, no results"]
        c1 -->|no| c2{"Step 2: Any declared type has all of its groups failed?"}
        c2 -->|yes| sk2["skipped (collection_failed). Not evaluated, no results"]
        c2 -->|no| eval["Evaluate the rule"]
        eval --> c3{"Step 3: Error during evaluation? (timeout, malformed fail element, resource_id not of target_type, resource_region not a string, no target_type resource with that id and region, ...)"}
        c3 -->|yes| err["error. No results"]
        c3 -->|no| c4{"Step 4: Any declared type has some failed groups (and some collected groups)?"}
        c4 -->|yes| inc["incomplete. Results from the collected groups"]
        c4 -->|no| c5{"Step 5: Any resource in the input (target_type or related_types) has a failure marker yata_error in its configuration?"}
        c5 -->|yes| inc2["incomplete. The resource is evaluated and listed in incomplete_resources"]
        c5 -->|no| ev["Step 6: evaluated. Results are produced"]
    end

    subgraph exit["Exit code (decided by yata-cli)"]
        s["Decide exit code"] --> z{"Interrupted (Ctrl-C)? Checked before the priority order"}
        z -->|yes| e130["Exit code 130. No report (stdout / --output untouched, temp file removed, reason to stderr)"]
        z -->|no| a{"Usage or input error? (bad arguments, options that cannot be combined, policy directory not specified or not found, manifest missing or incompatible, 0 rules in a directory, limit exceeded (directory-wide: manifest size, file count, directory count, depth; rule-level: rule file size, in commands other than rules test), symlink or reparse point found in the policy directory, rule load error, 0 rules after filtering, unsupported resource type declared by a filtered rule, snapshot validation error, collect output already exists without --overwrite)"}
        a -->|yes| e2["Exit code 2. No report (stdout / --output untouched, temp file removed, reason to stderr)"]
        a -->|no| b{"Fatal runtime error? (no credentials, GetCallerIdentity or DescribeRegions failure, partition other than aws, output write failure)"}
        b -->|yes| e3["Exit code 3. No report (reason to stderr). Exception: write failure during output. --output: temp file removed, existing file untouched. stdout: may be partially written"]
        b -->|no| c{"Finding at or above --fail-on?"}
        c -->|yes| e1["Exit code 1"]
        c -->|no| d{"Scan incomplete? (some collection failed, or any rule is incomplete / error / skipped, either reason)"}
        d -->|yes| e4["Exit code 4"]
        d -->|no| e0["Exit code 0"]
    end
```
