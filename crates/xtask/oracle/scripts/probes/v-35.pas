{ parity probe: v-35 }
unit P_v_35;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 1.5 + 'x';
  Show('1.5 + ''x''', v);
end;
end.
