{ parity probe: v-slot-23 }
unit P_v_slot_23;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Integer;
begin
  i := 1;
  v := i + '1';
  Show('i + ''1''', v);
end;
end.
