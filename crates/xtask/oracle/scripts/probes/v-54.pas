{ parity probe: v-54 }
unit P_v_54;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 2147483647 * 2;
  Show('2147483647 * 2', v);
end;
end.
