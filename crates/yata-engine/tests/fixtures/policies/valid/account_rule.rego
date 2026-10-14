package yata.rules.test_policies.account_rule

import rego.v1

metadata := {
	"title": "Account must not be flagged",
	"severity": "low",
	"description": "Fails for accounts flagged as bad.",
	"remediation": "Remove the flag.",
	"target_type": "aws.account.account",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := sprintf("Account %s is flagged", [input.id])
