package yata.rules.cli_ok_policies.nested.account_env

import rego.v1

metadata := {
	"title": "Account must not be production",
	"severity": "info",
	"description": "Fails for accounts tagged as production.",
	"remediation": "Use a separate account.",
	"target_type": "aws.account.account",
	"references": [],
}

fail contains finding if {
	some r in input.aws.account.account
	r.tags.env == "prod"
	finding := {"resource_id": r.id, "message": sprintf("Account %s is production", [r.id])}
}
