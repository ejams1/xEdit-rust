{ parity probe: v-19 }
unit P_v_19;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 5 < '6';
  Show('5 < ''6''', v);
end;
end.
