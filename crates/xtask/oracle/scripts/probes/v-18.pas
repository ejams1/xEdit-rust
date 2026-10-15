{ parity probe: v-18 }
unit P_v_18;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '5' = 5;
  Show('''5'' = 5', v);
end;
end.
