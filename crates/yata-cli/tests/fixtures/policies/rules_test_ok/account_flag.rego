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

fail contains finding if {
	some r in input.aws.account.account
	r.tags.flag == "bad"
	finding := {"resource_id": r.id, "message": sprintf("Account %s is flagged", [r.id])}
}
