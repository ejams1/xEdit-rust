{ parity probe: v-40 }
unit P_v_40;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 'x' = 'X';
  Show('''x'' = ''X''', v);
end;
end.
