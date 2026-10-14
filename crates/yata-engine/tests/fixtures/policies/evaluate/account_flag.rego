package yata.rules.eval_policies.account_flag

import rego.v1

metadata := {
	"title": "Account must not be flagged",
	"severity": "low",
	"description": "Fails the account resource when its flag is set.",
	"remediation": "Clear the flag.",
	"target_type": "aws.account.account",
	"references": [],
}

default fail := false

fail if input.configuration.Flagged == true

message := "Account is flagged"
