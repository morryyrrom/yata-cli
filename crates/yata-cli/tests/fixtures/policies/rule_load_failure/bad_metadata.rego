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

default fail := false

fail if input.id

message := "never evaluated"
