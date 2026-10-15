{ parity probe: v-12 }
unit P_v_12;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '2' * 3;
  Show('''2'' * 3', v);
end;
end.
