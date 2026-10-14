# Scan Sequence

```mermaid
sequenceDiagram
    actor User
    participant CLI as yata-cli
    participant Eng as yata-engine
    participant PD as Policy directory
    participant SD as Snapshot directory
    participant Aws as yata-provider-aws
    participant STS as STS / EC2
    participant Svc as Services
    participant Rep as yata-report

    Note over CLI,Rep: Exit 2/3/130: no report (reason to stderr).<br/>Exception: write failure during output →<br/>--output: temp file removed, existing file untouched / stdout: may be partially written.
    Note over Aws,Svc: Collection failures: a failed listing operation fails the whole group (CollectionError, no ResourceSet).<br/>A failed per-resource operation (e.g. GetBucketPolicy) puts a failure marker yata_error on that resource; the group still succeeds.<br/>First API region: with --profile, AWS_REGION, then the profile region, then us-east-1 (same as the AWS CLI). Without --profile, the SDK default resolution, then us-east-1.
    Note over CLI,SD: yata collect runs the same steps up to collection, then writes the Inventory as a snapshot.<br/>Output directory already exists (without --overwrite): Exit 2.<br/>Written to a temp directory next to the output, then renamed.<br/>Failure or interrupt: temp directory removed, existing output untouched.

    User->>CLI: yata scan
    CLI->>CLI: Parse arguments, init logging
    alt Options that cannot be combined (--snapshot with --profile, --region or --concurrency)
        CLI-->>User: Exit 2
    end
    Note over CLI,PD: Policies are loaded in every case, including --snapshot
    CLI->>CLI: Decide policy directories (--policy-dir, then YATA_POLICY_DIR)
    alt Not specified
        CLI-->>User: Exit 2
    end
    CLI->>Eng: load(dirs)
    Eng->>PD: Check each directory exists and is readable
    alt Not found or unreadable
        Eng-->>CLI: PolicyLoadError
        CLI-->>User: Exit 2
    end
    Eng->>PD: Read yata-policies.json
    PD-->>Eng: Manifest
    alt Manifest missing, malformed, too large, name invalid or duplicated, or format_version / min_yata_version does not match
        Eng-->>CLI: Error
        CLI-->>User: Exit 2
    end
    Eng->>PD: Scan down to the depth limit (checks limits on file count, directory count, depth, file size)
    PD-->>Eng: Libraries and rule files
    Eng->>Eng: Validate libraries and all rules (including rules to be filtered out)
    alt Limit exceeded for the directory, link found, library error, 0 rules in a directory, or rule-level failure
        Eng-->>CLI: Error
        CLI-->>User: Exit 2
    end
    Eng-->>CLI: All rules
    CLI->>Eng: Filter rules (--rule, --exclude-rule, --min-severity)
    Eng-->>CLI: Selected rules (target_type, related_types)
    alt 0 rules after filtering
        CLI-->>User: Exit 2
    end
    CLI->>Aws: Get supported resource types (incl. aws.account.account)
    Aws-->>CLI: Supported types
    CLI->>CLI: Check target_type and related_types of the selected rules only against supported types
    alt Unsupported type
        CLI-->>User: Exit 2
    end

    alt --snapshot given
        CLI->>SD: Read yata-snapshot.json and the files it lists (via yata-core)
        SD-->>CLI: Inventory
        CLI->>CLI: Validate (schema_version, listed files match, no unlisted files, every account x region x type is a ResourceSet or a CollectionError, one account only, limits, no links)
        alt Validation error
            CLI-->>User: SnapshotError, exit 2
        end
    else no --snapshot
        CLI->>Aws: Collect (resource types, profile, regions, concurrency)
        alt No credentials found
            Aws-->>CLI: Error (nothing is sent to STS)
            CLI-->>User: Exit 3
        else Credentials found
            Aws->>STS: sts:GetCallerIdentity
            alt GetCallerIdentity fails
                STS-->>Aws: Error
                Aws-->>CLI: Error
                CLI-->>User: Exit 3
            else Success
                STS-->>Aws: Account ID, ARN
                alt Partition of the ARN is not aws
                    Aws-->>CLI: SetupError
                    CLI-->>User: Exit 3
                end
                Aws->>Aws: Build account key and the aws.account.account resource
                opt --region not given
                    Aws->>STS: ec2:DescribeRegions
                    alt DescribeRegions fails
                        STS-->>Aws: Error
                        Aws-->>CLI: SetupError
                        CLI-->>User: Exit 3
                    else Success
                        STS-->>Aws: Enabled regions
                    end
                end
                par For each account x region x resource type (bounded concurrency)
                    Aws->>Svc: Describe / List / Get
                    alt Success
                        Svc-->>Aws: Resources, kept as a ResourceSet
                    else Failure
                        Svc-->>Aws: Error
                        Aws->>Aws: Record a CollectionError, continue
                    end
                end
                Aws-->>CLI: Inventory (ResourceSets incl. aws.account.account, CollectionErrors)
            end
        end
    end

    CLI->>Eng: Pass the whole Inventory
    Eng->>Eng: Per rule and account: build data (data.yata.context and data.aws.s3.bucket from target_type and related_types of all regions), decide status
    Eng->>Eng: Per target_type resource in id then region order: set input to the resource, eval fail (message and reason only when fail is true)
    Eng-->>CLI: RuleOutcome per rule and account (status + one pass / fail result per resource)
    CLI->>CLI: Build ScanReport (outcomes + collection_errors + metadata incl. policies)

    CLI->>CLI: Open output Write (stdout, or temp file in the --output directory)
    CLI->>Rep: ScanReport and Write
    alt write fails
        Rep-->>CLI: Write error
        CLI->>CLI: remove temp file (--output)
        CLI-->>User: Exit 3
    else written
        Rep-->>CLI: Written in the selected format
    end
    opt --output file
        CLI->>CLI: After writing, rename temp file to target
    end
    CLI->>CLI: Decide exit code
    CLI-->>User: Exit
```
