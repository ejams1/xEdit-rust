{ parity probe: v-60 }
unit P_v_60;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 1e308 * 10;
  Show('1e308 * 10', v);
end;
end.
