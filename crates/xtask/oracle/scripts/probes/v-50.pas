{ parity probe: v-50 }
unit P_v_50;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 1.5 mod 1;
  Show('1.5 mod 1', v);
end;
end.
