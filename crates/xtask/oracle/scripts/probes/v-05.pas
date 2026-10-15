{ parity probe: v-05 }
unit P_v_05;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 'x' + True;
  Show('''x'' + True', v);
end;
end.
