package yata.rules.cli_ok_policies.account_flag

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
