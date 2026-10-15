{ parity probe: x-124 }
unit P_x_124;

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
  AddMessage(IntToStr(Length(sl)));
  AddMessage('after');
end;
end.
