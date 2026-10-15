{ parity probe: v-02 }
unit P_v_02;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 5 + '1';
  Show('5 + ''1''', v);
end;
end.
