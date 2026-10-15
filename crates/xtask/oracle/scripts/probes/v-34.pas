{ parity probe: v-34 }
unit P_v_34;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 'x' + 1.5;
  Show('''x'' + 1.5', v);
end;
end.
