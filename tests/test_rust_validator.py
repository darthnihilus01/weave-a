import pytest
import weave_validator

def test_sql_validator_safe_select():
    sql = "SELECT id, name FROM customers WHERE notes = 'Urgent; call me'"
    res = weave_validator.validate_sql(sql, 100, 50)
    assert "LIMIT 50" in res
    assert "Urgent; call me" in res

def test_sql_validator_blocks_mutation():
    with pytest.raises(ValueError):
        weave_validator.validate_sql("DELETE FROM customers")

    with pytest.raises(ValueError):
        weave_validator.validate_sql("DROP TABLE customers")

def test_kotlin_validator_valid_code():
    code = """package com.genesis.app

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import com.genesis.app.ui.kit.Spacing

@Composable
fun GenesisApp() {
    Text("hi")
}
"""
    weave_validator.validate_kotlin_source(code, "com.genesis.app")
    assert weave_validator.has_genesis_app_entry(code) is True
    assert weave_validator.has_design_kit_import(code) is True

def test_kotlin_validator_blocks_runtime_exec():
    code = 'package com.genesis.app\nval p = Runtime.getRuntime().exec("ls")\n'
    with pytest.raises(ValueError, match="disallowed API"):
        weave_validator.validate_kotlin_source(code, "com.genesis.app")

def test_kotlin_validator_allows_comments_and_strings():
    code = 'package com.genesis.app\n// uses no ProcessBuilder\nval s = "Runtime.getRuntime() is not used"\n'
    weave_validator.validate_kotlin_source(code, "com.genesis.app")

def test_kotlin_validator_blocks_unapproved_imports():
    code = 'package com.genesis.app\nimport com.squareup.picasso.Picasso\n'
    with pytest.raises(ValueError, match="unapproved libraries"):
        weave_validator.validate_kotlin_source(code, "com.genesis.app")

def test_kotlin_validator_catches_interpolation_bypass():
    code = 'package com.genesis.app\nval s = """${Runtime.getRuntime().exec("ls")}"""\n'
    with pytest.raises(ValueError, match="disallowed API"):
        weave_validator.validate_kotlin_source(code, "com.genesis.app")

