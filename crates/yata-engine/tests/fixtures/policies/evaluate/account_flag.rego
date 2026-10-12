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

fail contains finding if {
	some a in input.aws.account.account
	a.configuration.Flagged == true
	finding := {"resource_id": a.id, "message": "Account is flagged"}
}
