{ parity probe: v-10 }
unit P_v_10;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '1' + '2';
  Show('''1'' + ''2''', v);
end;
end.
