package yata.rules.cli_load_failure_policies.bad_metadata

import rego.v1

metadata := {
	"title": "Rule with an invalid severity",
	"severity": "urgent",
	"description": "The severity is not one of the allowed values.",
	"remediation": "None.",
	"target_type": "aws.account.account",
	"references": [],
}

fail contains finding if {
	some r in input.aws.account.account
	finding := {"resource_id": r.id, "message": "never evaluated"}
}
