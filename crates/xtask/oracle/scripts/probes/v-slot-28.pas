{ parity probe: v-slot-28 }
unit P_v_slot_28;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := StrToInt('5') + '5';
  Show('StrToInt(''5'') + ''5''', v);
end;
end.
