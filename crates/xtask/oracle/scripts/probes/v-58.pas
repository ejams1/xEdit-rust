{ parity probe: v-58 }
unit P_v_58;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 1 mod 0;
  Show('1 mod 0', v);
end;
end.
