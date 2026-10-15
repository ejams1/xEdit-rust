{ parity probe: v-slot-27 }
unit P_v_slot_27;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := IntToStr(5) + 5;
  Show('IntToStr(5) + 5', v);
end;
end.
