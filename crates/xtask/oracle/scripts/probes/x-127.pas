{ parity probe: x-127 }
unit P_x_127;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage(IntToStr(Length(1.5)));
  AddMessage('after');
end;
end.
