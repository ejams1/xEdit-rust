{ parity probe: x-125 }
unit P_x_125;

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
  AddMessage(IntToStr(Length(nil)));
  AddMessage('after');
end;
end.
