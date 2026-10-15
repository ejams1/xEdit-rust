{ parity probe: v-29 }
unit P_v_29;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 5 + 'x';
  Show('5 + ''x''', v);
end;
end.
