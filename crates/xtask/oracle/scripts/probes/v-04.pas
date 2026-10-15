{ parity probe: v-04 }
unit P_v_04;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 'x' + 5;
  Show('''x'' + 5', v);
end;
end.
