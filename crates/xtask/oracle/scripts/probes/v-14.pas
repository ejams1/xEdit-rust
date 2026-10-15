{ parity probe: v-14 }
unit P_v_14;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '6' / 2;
  Show('''6'' / 2', v);
end;
end.
