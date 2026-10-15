{ parity probe: v-13 }
unit P_v_13;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '6' - 1;
  Show('''6'' - 1', v);
end;
end.
