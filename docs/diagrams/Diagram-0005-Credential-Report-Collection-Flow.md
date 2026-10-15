# Credential Report Collection Flow (Issue 5)

```mermaid
flowchart TD
    start(["Collect aws.iam.user in one account"]) --> lu["Call ListUsers"]
    lu --> luf{"ListUsers failed?"}
    luf -->|yes| ce["CollectionError (for example access_denied): the whole aws.iam.user group fails"]
    luf -->|no| cnt{"Are there 0 users?"}
    cnt -->|yes| empty(["Return an empty array. Do not call GenerateCredentialReport or GetCredentialReport"])
    cnt -->|no| gen["Call GenerateCredentialReport (runs at the same time as the per-user calls)"]
    gen --> genf{"Call failed? (for example access_denied)"}
    genf -->|yes| mark["Put yata_error on the GetCredentialReport key of every user"]
    genf -->|no| st{"State is COMPLETE?"}
    st -->|no, still in progress| wait["Wait for the interval, then call GenerateCredentialReport again"]
    wait --> gen
    st -->|no, time limit reached| mark
    st -->|yes| get["Call GetCredentialReport"]
    get --> getf{"Call failed? (for example ReportNotPresent)"}
    getf -->|yes| mark
    getf -->|no| parse{"Output checks or CSV parse failed?"}
    parse -->|yes| mark
    parse -->|no| match{"Is there a row whose arn equals the user ARN?"}
    match -->|"exactly one"| ok["Set the GetCredentialReport key of the user to an object with GeneratedTime, ReportFormat and Content (the row as column name to value)"]
    match -->|none| missing["Put yata_error on the GetCredentialReport key of that user only (invalid_response, MissingUserRow)"]
    match -->|"two or more"| dup["Put yata_error on the GetCredentialReport key of that user only (invalid_response, DuplicateArn)"]
    mark --> inc(["Rules 2.10 to 2.13 become incomplete (the ListUsers result stays a success)"])

    subgraph notes ["Notes"]
        n_reuse["IAM reuses a report made within the last 4 hours, so the first call may already return COMPLETE"]
        n_polling["Wait interval, time limit, and the failure kinds for each State: see Diagram-0013-Credential-Report-Polling (Feature-0013)"]
        n_rows["Output checks, CSV parse failures, and row matching: see Diagram-0013-Credential-Report-Row-Mapping (Feature-0013)"]
        n_join["The per-user calls and the report run at the same time: see Diagram-0013-User-Collector-Concurrency (Feature-0013)"]
        n_root["The root_account row is not put on any resource in this scope (temporary gap; the destination is decided when 2.7 is added). 2.4 and 2.5 use GetAccountSummary"]
    end
```
