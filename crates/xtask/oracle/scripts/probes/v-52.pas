{ parity probe: v-52 }
unit P_v_52;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 2147483647 + 1;
  Show('2147483647 + 1', v);
end;
end.
